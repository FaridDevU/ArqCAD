//! Decodes SHX glyph bytecode into [`GlyphOutline`].
//!
//! The virtual-pen machine uses special opcodes 0 through 14 and
//! length-direction vectors. Octant and fractional arcs become exact bulge arcs
//! split into pieces no larger than `MAX_ARC_SEG`.
//!
//! Truncated commands preserve the outline decoded so far. Subshape recursion
//! and total operations are bounded to reject pathological input without panic.

use std::collections::BTreeMap;

use super::{GlyphOutline, PolyPoint, PolySeg};

/// Maximum radians in one emitted bulge arc. This stays below 180 degrees while
/// allowing a 90-degree arc in one segment.
const MAX_ARC_SEG: f64 = 2.0;

/// Maximum nesting depth for subshape opcode 7.
const MAX_SUBSHAPE_DEPTH: u32 = 10;

/// Global operation budget per glyph to bound subshape expansion.
const OP_BUDGET: u32 = 100_000;

/// Position-stack limit for opcodes 5 and 6.
const MAX_STACK: usize = 64;

/// Unit displacement table for the 16 vector directions.
///
/// The dominant component is 1.0 and diagonal directions use +/-0.5 on the
/// minor axis. The low nibble indexes counterclockwise 22.5-degree steps from
/// east.
const DIRS: [(f64, f64); 16] = [
    (1.0, 0.0),   // 0   0°   E
    (1.0, 0.5),   // 1   22.5°
    (1.0, 1.0),   // 2   45°  NE
    (0.5, 1.0),   // 3   67.5°
    (0.0, 1.0),   // 4   90°  N
    (-0.5, 1.0),  // 5   112.5°
    (-1.0, 1.0),  // 6   135° NW
    (-1.0, 0.5),  // 7   157.5°
    (-1.0, 0.0),  // 8   180° W
    (-1.0, -0.5), // 9   202.5°
    (-1.0, -1.0), // A   225° SW
    (-0.5, -1.0), // B   247.5°
    (0.0, -1.0),  // C   270° S
    (0.5, -1.0),  // D   292.5°
    (1.0, -1.0),  // E   315° SE
    (1.0, -0.5),  // F   337.5°
];

/// Decodes `code`, resolves subshapes, and normalizes coordinates by `above`.
pub(super) fn decode_glyph(code: u16, glyphs: &BTreeMap<u16, &[u8]>, above: f64) -> GlyphOutline {
    let mut dec = Decoder {
        glyphs,
        scale: 1.0,
        x: 0.0,
        y: 0.0,
        pen_down: true, // Drawing mode is active when a shape starts.
        stack: Vec::new(),
        stroke: Vec::new(),
        strokes: Vec::new(),
        budget: OP_BUDGET,
    };
    if let Some(body) = glyphs.get(&code) {
        dec.run(body, 0);
    }
    dec.flush();

    let inv = if above != 0.0 { 1.0 / above } else { 1.0 };
    let strokes = dec
        .strokes
        .into_iter()
        .map(|seg| PolySeg {
            points: seg
                .points
                .into_iter()
                .map(|p| PolyPoint {
                    x: (f64::from(p.x) * inv) as f32,
                    y: (f64::from(p.y) * inv) as f32,
                    bulge: p.bulge,
                })
                .collect(),
        })
        .collect();

    GlyphOutline {
        strokes,
        advance: (dec.x * inv) as f32,
    }
}

/// Virtual-pen state while decoding a glyph.
struct Decoder<'a> {
    glyphs: &'a BTreeMap<u16, &'a [u8]>,
    /// Accumulated raw length scale from opcodes 3 and 4.
    scale: f64,
    x: f64,
    y: f64,
    pen_down: bool,
    stack: Vec<(f64, f64)>,
    /// Stroke under construction in raw coordinates.
    stroke: Vec<PolyPoint>,
    strokes: Vec<PolySeg>,
    budget: u32,
}

impl Decoder<'_> {
    /// Executes bytecode with `depth` bounding subshape recursion.
    fn run(&mut self, code: &[u8], depth: u32) {
        let mut c = ByteCursor { data: code, pos: 0 };
        let mut skip_next = false;
        while let Some(op) = parse_op(&mut c) {
            if self.budget == 0 {
                return;
            }
            self.budget -= 1;

            if matches!(op, Op::End) {
                return;
            }
            if skip_next {
                // Opcode 14 marks the next command for omission in horizontal mode.
                skip_next = false;
                continue;
            }
            if matches!(op, Op::Vertical) {
                skip_next = true;
                continue;
            }
            self.apply(op, depth);
        }
    }

    fn apply(&mut self, op: Op, depth: u32) {
        match op {
            Op::End | Op::Vertical | Op::Noop => {}
            Op::PenDown => self.pen_down = true,
            Op::PenUp => {
                self.pen_down = false;
                self.flush();
            }
            Op::DivScale(d) => {
                if d != 0 {
                    self.scale /= f64::from(d);
                }
            }
            Op::MulScale(m) => self.scale *= f64::from(m),
            Op::Push => {
                if self.stack.len() < MAX_STACK {
                    self.stack.push((self.x, self.y));
                }
            }
            Op::Pop => {
                if let Some((px, py)) = self.stack.pop() {
                    self.flush();
                    self.x = px;
                    self.y = py;
                }
            }
            Op::SubShape(sub) => {
                if depth < MAX_SUBSHAPE_DEPTH
                    && let Some(&body) = self.glyphs.get(&sub)
                {
                    self.run(body, depth + 1);
                }
            }
            Op::Vector(len, dir) => {
                let (dx, dy) = DIRS[dir as usize];
                let l = f64::from(len) * self.scale;
                self.line_to(self.x + dx * l, self.y + dy * l);
            }
            Op::Move(dx, dy) => {
                self.line_to(
                    self.x + f64::from(dx) * self.scale,
                    self.y + f64::from(dy) * self.scale,
                );
            }
            Op::PolyMove(pairs) => {
                for (dx, dy) in pairs {
                    self.line_to(
                        self.x + f64::from(dx) * self.scale,
                        self.y + f64::from(dy) * self.scale,
                    );
                }
            }
            Op::Bulge(dx, dy, h) => {
                self.bulge_to(
                    self.x + f64::from(dx) * self.scale,
                    self.y + f64::from(dy) * self.scale,
                    f64::from(h) / 127.0,
                );
            }
            Op::PolyBulge(triples) => {
                for (dx, dy, h) in triples {
                    self.bulge_to(
                        self.x + f64::from(dx) * self.scale,
                        self.y + f64::from(dy) * self.scale,
                        f64::from(h) / 127.0,
                    );
                }
            }
            Op::Octant { radius, spec } => {
                let (start, sweep, r) = octant_geom(f64::from(radius) * self.scale, spec);
                self.arc(start, sweep, r);
            }
            Op::Frac {
                start_off,
                end_off,
                radius,
                spec,
            } => {
                let (start, sweep, r) =
                    frac_geom(f64::from(radius) * self.scale, start_off, end_off, spec);
                self.arc(start, sweep, r);
            }
        }
    }

    /// Emits a straight segment or moves the raised pen to `(nx, ny)`.
    fn line_to(&mut self, nx: f64, ny: f64) {
        self.seg_to(nx, ny, 0.0);
    }

    /// Emits or moves through a bulge segment to `(nx, ny)`.
    fn bulge_to(&mut self, nx: f64, ny: f64, bulge: f64) {
        self.seg_to(nx, ny, bulge);
    }

    /// Extends the current stroke while the pen is down; otherwise closes the
    /// stroke and moves the pen.
    fn seg_to(&mut self, nx: f64, ny: f64, bulge: f64) {
        if self.pen_down {
            if self.stroke.is_empty() {
                self.stroke.push(PolyPoint {
                    x: self.x as f32,
                    y: self.y as f32,
                    bulge: 0.0,
                });
            }
            self.stroke.push(PolyPoint {
                x: nx as f32,
                y: ny as f32,
                bulge: bulge as f32,
            });
        } else {
            self.flush();
        }
        self.x = nx;
        self.y = ny;
    }

    /// Emits an arc in pieces no larger than `MAX_ARC_SEG`. `start` is the
    /// current point angle and positive `sweep` is counterclockwise.
    fn arc(&mut self, start: f64, sweep: f64, r: f64) {
        if !r.is_finite() || r <= 0.0 || !sweep.is_finite() || sweep == 0.0 {
            return;
        }
        // The current point lies on the circle at `start`.
        let cx = self.x - r * start.cos();
        let cy = self.y - r * start.sin();
        let n = (sweep.abs() / MAX_ARC_SEG).ceil().max(1.0);
        let steps = n as u32;
        let step = sweep / n;
        let seg_bulge = (step / 4.0).tan();
        for i in 1..=steps {
            let a = start + step * f64::from(i);
            let nx = cx + r * a.cos();
            let ny = cy + r * a.sin();
            self.seg_to(nx, ny, seg_bulge);
        }
    }

    /// Closes and stores a stroke with at least two vertices.
    fn flush(&mut self) {
        if self.stroke.len() >= 2 {
            self.strokes.push(PolySeg {
                points: core::mem::take(&mut self.stroke),
            });
        } else {
            self.stroke.clear();
        }
    }
}

/// Derives `(start_angle, sweep, radius)` for octant-arc opcode 10.
///
/// In `spec`, bit 7 selects clockwise direction, bits 4 through 6 select the
/// starting octant, and bits 0 through 2 count 45-degree octants. Zero means 8.
fn octant_geom(r: f64, spec: u8) -> (f64, f64, f64) {
    let cw = spec & 0x80 != 0;
    let s = (spec >> 4) & 0x07;
    let c = spec & 0x07;
    let count = if c == 0 { 8 } else { u32::from(c) };
    let start = f64::from(s) * std::f64::consts::FRAC_PI_4;
    let mag = f64::from(count) * std::f64::consts::FRAC_PI_4;
    let sweep = if cw { -mag } else { mag };
    (start, sweep, r)
}

/// Derives `(start_angle, sweep, radius)` for fractional-arc opcode 11.
///
/// `start_off` and `end_off` are 1/256-octant fractions that shift the endpoints.
/// Zero offsets reduce exactly to the equivalent octant arc.
fn frac_geom(r: f64, start_off: u8, end_off: u8, spec: u8) -> (f64, f64, f64) {
    let cw = spec & 0x80 != 0;
    let s = (spec >> 4) & 0x07;
    let c = spec & 0x07;
    let oct = std::f64::consts::FRAC_PI_4;
    let sf = f64::from(start_off) / 256.0;
    let ef = f64::from(end_off) / 256.0;
    let start = (f64::from(s) + sf) * oct;
    let mag = (f64::from(c) + (ef - sf)) * oct;
    let sweep = if cw { -mag } else { mag };
    (start, sweep, r)
}

/// Parsed command with operands consumed from bytecode.
enum Op {
    End,
    PenDown,
    PenUp,
    DivScale(u8),
    MulScale(u8),
    Push,
    Pop,
    SubShape(u16),
    Move(i8, i8),
    PolyMove(Vec<(i8, i8)>),
    Octant {
        radius: u8,
        spec: u8,
    },
    Frac {
        start_off: u8,
        end_off: u8,
        radius: u16,
        spec: u8,
    },
    Bulge(i8, i8, i8),
    PolyBulge(Vec<(i8, i8, i8)>),
    Vertical,
    /// Vector byte containing length 1-15 and direction 0-15.
    Vector(u8, u8),
    /// Byte 0x0F or an unrecognized opcode; no effect.
    Noop,
}

/// Bytecode cursor. Exhaustion returns `None` and ends the glyph with any partial
/// outline preserved.
struct ByteCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl ByteCursor<'_> {
    fn u8(&mut self) -> Option<u8> {
        let b = self.data.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn i8(&mut self) -> Option<i8> {
        self.u8().map(|b| b as i8)
    }
}

/// Parses the next command and consumes its operands. Returns `None` at the end
/// or in the middle of a truncated command.
fn parse_op(c: &mut ByteCursor) -> Option<Op> {
    let b = c.u8()?;
    let hi = b >> 4;
    let lo = b & 0x0F;
    if hi != 0 {
        // Vector byte: high nibble is length, low nibble is direction.
        return Some(Op::Vector(hi, lo));
    }
    Some(match lo {
        0 => Op::End,
        1 => Op::PenDown,
        2 => Op::PenUp,
        3 => Op::DivScale(c.u8()?),
        4 => Op::MulScale(c.u8()?),
        5 => Op::Push,
        6 => Op::Pop,
        7 => Op::SubShape(u16::from(c.u8()?)),
        8 => Op::Move(c.i8()?, c.i8()?),
        9 => {
            let mut pairs = Vec::new();
            loop {
                let dx = c.i8()?;
                let dy = c.i8()?;
                if dx == 0 && dy == 0 {
                    break;
                }
                pairs.push((dx, dy));
            }
            Op::PolyMove(pairs)
        }
        10 => Op::Octant {
            radius: c.u8()?,
            spec: c.u8()?,
        },
        11 => {
            let start_off = c.u8()?;
            let end_off = c.u8()?;
            let hi_r = c.u8()?;
            let lo_r = c.u8()?;
            let spec = c.u8()?;
            Op::Frac {
                start_off,
                end_off,
                radius: (u16::from(hi_r) << 8) | u16::from(lo_r),
                spec,
            }
        }
        12 => Op::Bulge(c.i8()?, c.i8()?, c.i8()?),
        13 => {
            let mut triples = Vec::new();
            loop {
                let dx = c.i8()?;
                let dy = c.i8()?;
                if dx == 0 && dy == 0 {
                    break;
                }
                let h = c.i8()?;
                triples.push((dx, dy, h));
            }
            Op::PolyBulge(triples)
        }
        14 => Op::Vertical,
        _ => Op::Noop, // lo == 15 (byte 0x0F)
    })
}
