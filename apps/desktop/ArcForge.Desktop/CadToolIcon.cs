using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;

namespace ArcForge.Desktop;

public sealed class CadToolIcon : Control
{
    public static readonly StyledProperty<string> ToolProperty =
        AvaloniaProperty.Register<CadToolIcon, string>(nameof(Tool), "Line");

    public static readonly StyledProperty<Color> AccentProperty =
        AvaloniaProperty.Register<CadToolIcon, Color>(nameof(Accent), Color.Parse("#D0D6DC"));

    // Detail colour stays intentionally muted; toggle styles still override it per state.
    public static readonly StyledProperty<Color> HighlightProperty =
        AvaloniaProperty.Register<CadToolIcon, Color>(nameof(Highlight), Color.Parse("#6E8AA0"));

    static CadToolIcon()
    {
        AffectsRender<CadToolIcon>(ToolProperty, AccentProperty, HighlightProperty);
    }

    public CadToolIcon()
    {
        Width = 28;
        Height = 28;
        IsHitTestVisible = false;
    }

    public string Tool
    {
        get => GetValue(ToolProperty);
        set => SetValue(ToolProperty, value);
    }

    public Color Accent
    {
        get => GetValue(AccentProperty);
        set => SetValue(AccentProperty, value);
    }

    public Color Highlight
    {
        get => GetValue(HighlightProperty);
        set => SetValue(HighlightProperty, value);
    }

    public override void Render(DrawingContext context)
    {
        base.Render(context);

        var size = Math.Min(Bounds.Width, Bounds.Height);
        if (size <= 0)
        {
            return;
        }

        // Most glyphs occupy coordinates 2..26 inside the 28-unit design box.
        // Scale that optical box, not the empty outer box, so 16/24 px slots stay legible.
        var scale = size / 25d;
        var left = (Bounds.Width - (28 * scale)) / 2;
        var top = (Bounds.Height - (28 * scale)) / 2;
        var compact = size < 20;
        var detailed = size >= 23;
        var presentation = size >= 31;
        var accent = new SolidColorBrush(Accent);
        var quiet = new SolidColorBrush(Color.Parse("#9CA6AF"));
        var blue = new SolidColorBrush(Highlight);
        var warm = new SolidColorBrush(Color.Parse("#B59A68"));
        var green = new SolidColorBrush(Color.Parse("#789B84"));
        var wash = new SolidColorBrush(Color.Parse("#31383F"));
        var surface = new SolidColorBrush(Color.Parse("#252C33"));
        var blueWash = new SolidColorBrush(Color.Parse("#32404C"));
        var pen = new Pen(accent, Math.Clamp(1.75 * scale, 1.15, 2.1));
        var fine = new Pen(quiet, Math.Clamp(1.3 * scale, 0.9, 1.5));
        var micro = new Pen(quiet, Math.Clamp(1.0 * scale, 0.78, 1.2));
        var bluePen = new Pen(blue, Math.Clamp(1.4 * scale, 0.95, 1.65));
        var dash = new Pen(quiet, Math.Clamp(1.2 * scale, 0.85, 1.4),
            new DashStyle(compact ? [1.5, 1.5] : [2.2, 2.2], 0));

        Point P(double x, double y) => new(left + (x * scale), top + (y * scale));
        Rect R(double x, double y, double width, double height) =>
            new(left + (x * scale), top + (y * scale), width * scale, height * scale);
        void Line(Pen stroke, double x1, double y1, double x2, double y2) =>
            context.DrawLine(stroke, P(x1, y1), P(x2, y2));
        void Poly(Pen stroke, params double[] points)
        {
            for (var i = 0; i + 3 < points.Length; i += 2)
            {
                Line(stroke, points[i], points[i + 1], points[i + 2], points[i + 3]);
            }
        }
        void Rect(Pen stroke, double x, double y, double width, double height) =>
            context.DrawRectangle(null, stroke, R(x, y, width, height));
        void Ellipse(Pen stroke, double cx, double cy, double rx, double ry) =>
            context.DrawEllipse(null, stroke, P(cx, cy), rx * scale, ry * scale);
        void Dot(double x, double y, double radius = 1.45) =>
            context.DrawEllipse(blue, null, P(x, y), radius * scale, radius * scale);
        void Box(double x, double y, double side = 3) =>
            context.DrawRectangle(blue, null, R(x - (side / 2), y - (side / 2), side, side));
        void Shape(IBrush? fill, Pen? stroke, params double[] points)
        {
            var geometry = new StreamGeometry();
            using (var path = geometry.Open())
            {
                path.BeginFigure(P(points[0], points[1]), fill is not null);
                for (var i = 2; i + 1 < points.Length; i += 2)
                {
                    path.LineTo(P(points[i], points[i + 1]));
                }
                path.EndFigure(true);
            }
            context.DrawGeometry(fill, stroke, geometry);
        }
        void Curve(Pen stroke, double sx, double sy, double c1x, double c1y,
            double c2x, double c2y, double ex, double ey)
        {
            var geometry = new StreamGeometry();
            using (var path = geometry.Open())
            {
                path.BeginFigure(P(sx, sy), false);
                path.CubicBezierTo(P(c1x, c1y), P(c2x, c2y), P(ex, ey));
                path.EndFigure(false);
            }
            context.DrawGeometry(null, stroke, geometry);
        }

        // ponytail: one native renderer keeps the small CAD set crisp without an icon framework or asset pipeline.
        switch ((Tool ?? string.Empty).Trim().ToLowerInvariant())
        {
            case "line":
                Line(pen, 5, 22, 23, 6);
                Box(5, 22, compact ? 2.4 : 3);
                Box(23, 6, compact ? 2.4 : 3);
                break;

            case "polyline":
                Poly(pen, 4, 21, 9, 7, 16, 14, 24, 5);
                Box(4, 21, compact ? 2.2 : 2.7);
                Box(9, 7, compact ? 2.2 : 2.7);
                Box(16, 14, compact ? 2.2 : 2.7);
                Box(24, 5, compact ? 2.2 : 2.7);
                break;

            case "circle":
                Ellipse(pen, 14, 14, 9.5, 9.5);
                Dot(14, 14, 1.25);
                if (detailed)
                {
                    Line(fine, 10.5, 14, 17.5, 14);
                    Line(fine, 14, 10.5, 14, 17.5);
                }
                break;

            case "arc":
                Curve(pen, 5, 22, 5, 12, 9, 4, 14, 4);
                Curve(pen, 14, 4, 20, 4, 23, 8, 23, 14);
                Box(5, 22, compact ? 2.2 : 2.7);
                Box(23, 14, compact ? 2.2 : 2.7);
                if (detailed)
                {
                    Dot(14, 4, 1.05);
                }
                break;

            case "rectangle":
                Rect(pen, 5, 6, 18, 16);
                Box(5, 22, compact ? 2.2 : 2.7);
                Box(23, 6, compact ? 2.2 : 2.7);
                break;

            case "hatch":
                Rect(fine, 5, 5, 18, 18);
                Poly(pen, 5, 11, 11, 5);
                if (!compact)
                {
                    Poly(pen, 5, 17, 17, 5);
                    Poly(pen, 11, 23, 23, 11);
                }
                Poly(pen, 5, 23, 23, 5);
                Poly(pen, 17, 23, 23, 17);
                break;

            case "boundary":
                Shape(blueWash, fine, 4, 8, 8, 4, 19, 5, 24, 10, 22, 21, 17, 24, 7, 22, 4, 16);
                Rect(pen, 8, 9, 12, 10);
                if (detailed)
                {
                    Line(micro, 10, 12, 18, 12);
                    Line(micro, 10, 15.5, 18, 15.5);
                }
                Box(8, 19, compact ? 2.5 : 3);
                Box(20, 9, compact ? 2.5 : 3);
                break;

            case "move":
                Line(pen, 14, 4, 14, 24);
                Line(pen, 4, 14, 24, 14);
                Shape(blue, null, 14, 3, 11, 7, 17, 7);
                Shape(blue, null, 25, 14, 21, 11, 21, 17);
                Shape(blue, null, 14, 25, 11, 21, 17, 21);
                Shape(blue, null, 3, 14, 7, 11, 7, 17);
                context.DrawRectangle(wash, pen, R(11, 11, 6, 6));
                break;

            case "copy":
                context.DrawRectangle(surface, fine, R(5, 4, 13, 14));
                context.DrawRectangle(wash, pen, R(8, 9, 15, 15));
                Line(bluePen, 11.5, 16.5, 19.5, 16.5);
                Line(bluePen, 15.5, 12.5, 15.5, 20.5);
                break;

            case "rotate":
                Curve(pen, 5, 17, 5, 7, 15, 3, 22, 9);
                Curve(pen, 22, 9, 24, 14, 21, 22, 14, 23);
                Shape(blue, null, 22, 9, 18, 8, 21, 13);
                if (detailed)
                {
                    Shape(surface, fine, 9, 13, 15, 9, 19, 15, 13, 19);
                }
                break;

            case "trim":
                Line(fine, 5, 5, 23, 23);
                Line(pen, 5, 23, 11, 17);
                Line(pen, 17, 11, 23, 5);
                Line(bluePen, 9, 14.5, 13, 18.5);
                Line(bluePen, 15, 9.5, 19, 13.5);
                Dot(14, 14, 1);
                break;

            case "offset":
                Poly(fine, 5, 7, 16, 7, 23, 14, 23, 21);
                Poly(pen, 5, 13, 13, 13, 17, 17, 17, 23);
                Line(bluePen, 13, 9, 17, 13);
                Shape(blue, null, 18, 14, 13.5, 13, 17, 9.5);
                break;

            case "mirror":
                Line(dash, 14, 3, 14, 25);
                Shape(wash, pen, 4, 22, 10, 6, 12, 22);
                Shape(null, fine, 24, 22, 18, 6, 16, 22);
                break;

            case "array":
                for (var row = 0; row < 3; row++)
                {
                    for (var column = 0; column < 3; column++)
                    {
                        context.DrawRectangle(column == 0 && row == 0 ? wash : null,
                            column == 0 && row == 0 ? pen : fine,
                            R(4 + (column * 8), 4 + (row * 8), 5, 5));
                    }
                }
                break;

            case "stretch":
                Rect(fine, 4, 7, 12, 14);
                Line(dash, 12, 4, 12, 24);
                Poly(pen, 12, 7, 23, 7, 23, 21, 12, 21);
                Line(bluePen, 14, 14, 21, 14);
                Shape(blue, null, 23, 14, 19, 11, 19, 17);
                break;

            case "fillet":
                Line(dash, 6, 4, 6, 22);
                Line(dash, 6, 22, 24, 22);
                Curve(pen, 6, 9, 6, 17, 11, 22, 19, 22);
                Dot(6, 9, compact ? 1.2 : 1.45);
                Dot(19, 22, compact ? 1.2 : 1.45);
                if (presentation)
                {
                    Curve(micro, 9, 7, 12, 10, 15, 12, 20, 12);
                }
                break;

            case "scale":
                context.DrawRectangle(surface, fine, R(4, 14, 9, 9));
                context.DrawRectangle(wash, pen, R(10, 5, 13, 13));
                Line(bluePen, 6, 22, 22, 6);
                Shape(blue, null, 22, 6, 16.5, 7, 21, 11.5);
                if (detailed)
                {
                    Box(10, 18, 2.5);
                    Box(23, 5, 2.5);
                }
                break;

            case "text":
                Poly(pen, 5, 22, 12, 5, 19, 22);
                Line(pen, 8, 15, 16, 15);
                Line(bluePen, 20, 9, 24, 9);
                if (detailed)
                {
                    Line(fine, 20, 14, 25, 14);
                    Line(fine, 20, 19, 23, 19);
                }
                break;

            case "dimension":
                Line(fine, 5, 4, 5, 24);
                Line(fine, 23, 4, 23, 24);
                Line(pen, 5, 14, 23, 14);
                Shape(blue, null, 5, 14, 9, 11.5, 9, 16.5);
                Shape(blue, null, 23, 14, 19, 11.5, 19, 16.5);
                if (detailed)
                {
                    Line(fine, 5, 23, 23, 23);
                }
                break;

            case "table":
                context.DrawRectangle(wash, pen, R(1, 4.5, 23.5, 19.5));
                Line(pen, 1, 10, 24.5, 10);
                Line(fine, 1, 16.75, 24.5, 16.75);
                Line(fine, 11, 10, 11, 24);
                if (!compact)
                {
                    Line(fine, 18, 10, 18, 24);
                }
                break;

            case "leader":
                Poly(pen, 4, 22, 11, 12, 24, 12);
                Shape(blue, null, 4, 22, 5.5, 17.5, 8, 20.5);
                if (detailed)
                {
                    Line(fine, 14, 17, 24, 17);
                    Line(fine, 14, 21, 21, 21);
                }
                break;

            case "layer":
                Shape(wash, pen, 4, 9, 14, 4, 24, 9, 14, 14);
                Poly(fine, 4, 14, 14, 19, 24, 14);
                Poly(fine, 4, 19, 14, 24, 24, 19);
                if (detailed)
                {
                    Dot(14, 9, 1.1);
                }
                break;

            case "layerproperties":
            case "layer-properties":
                Shape(wash, pen, 3, 7, 11, 3, 19, 7, 11, 11);
                Poly(fine, 3, 11, 11, 15, 19, 11);
                Poly(fine, 3, 15, 11, 19, 19, 15);
                Line(micro, 20, 5, 25, 5);
                Line(micro, 20, 10, 25, 10);
                Line(micro, 20, 15, 25, 15);
                Dot(22, 5, 1.15);
                Dot(24, 10, 1.15);
                Dot(21, 15, 1.15);
                break;

            case "block":
                Rect(pen, 5, 5, 8, 8);
                Rect(fine, 15, 5, 8, 8);
                Rect(fine, 5, 15, 8, 8);
                context.DrawRectangle(blueWash, bluePen, R(15, 15, 8, 8));
                Line(fine, 10, 14, 18, 14);
                Line(fine, 14, 10, 14, 18);
                break;

            case "insert":
                context.DrawRectangle(surface, fine, R(4, 8, 13, 13));
                context.DrawRectangle(wash, pen, R(10, 4, 13, 13));
                Line(bluePen, 20, 19, 20, 25);
                Shape(blue, null, 20, 25, 16.5, 21, 23.5, 21);
                if (detailed)
                {
                    Line(micro, 13, 8, 20, 8);
                    Line(micro, 13, 12, 18, 12);
                }
                break;

            case "attribute":
                Shape(surface, pen, 4, 8, 10, 3, 24, 3, 24, 20, 18, 25, 4, 25);
                Dot(9, 8, 1.25);
                Poly(fine, 10, 21, 14, 9, 18, 21);
                Line(fine, 11.8, 16, 16.2, 16);
                break;

            case "changebase":
            case "change-base":
                context.DrawRectangle(wash, fine, R(5, 5, 16, 14));
                Line(micro, 5, 12, 21, 12);
                Line(micro, 13, 5, 13, 19);
                Line(bluePen, 13, 16, 13, 25);
                Line(bluePen, 8, 20.5, 18, 20.5);
                context.DrawRectangle(surface, bluePen, R(11.5, 19, 3, 3));
                Shape(blueWash, null, 13, 25, 10, 21.5, 16, 21.5);
                break;

            case "measure":
                Shape(wash, pen, 4, 19, 19, 4, 24, 9, 9, 24);
                Line(fine, 9, 17, 12, 20);
                Line(fine, 12, 14, 15, 17);
                Line(fine, 15, 11, 18, 14);
                Line(fine, 18, 8, 21, 11);
                break;

            case "properties":
                Line(fine, 4, 7, 24, 7);
                Line(fine, 4, 14, 24, 14);
                Line(fine, 4, 21, 24, 21);
                context.DrawEllipse(blueWash, bluePen, P(10, 7), 2.4 * scale, 2.4 * scale);
                context.DrawEllipse(blueWash, bluePen, P(19, 14), 2.4 * scale, 2.4 * scale);
                context.DrawEllipse(blueWash, bluePen, P(13, 21), 2.4 * scale, 2.4 * scale);
                break;

            case "matchproperties":
            case "match-properties":
                Shape(surface, fine, 4, 19, 8, 23, 15, 16, 11, 12);
                Shape(wash, pen, 10, 12, 18, 4, 23, 9, 15, 17);
                Line(bluePen, 4, 24, 13, 24);
                Line(bluePen, 17, 20, 24, 20);
                if (detailed)
                {
                    Line(micro, 15, 8, 19, 12);
                    Dot(23, 20, 1.1);
                }
                break;

            case "color":
                Shape(surface, pen, 4, 15, 5, 7, 12, 3, 20, 5, 24, 11, 22, 17, 17, 20, 13, 18, 11, 23, 6, 22);
                context.DrawEllipse(blue, null, P(9, 8), 1.8 * scale, 1.8 * scale);
                context.DrawEllipse(green, null, P(15, 7), 1.8 * scale, 1.8 * scale);
                context.DrawEllipse(warm, null, P(20, 11), 1.8 * scale, 1.8 * scale);
                context.DrawEllipse(accent, null, P(8, 15), 1.8 * scale, 1.8 * scale);
                if (presentation)
                {
                    context.DrawEllipse(quiet, null, P(16, 14), 1.2 * scale, 1.2 * scale);
                }
                break;

            case "linetype":
            case "line-type":
                Line(pen, 4, 7, 24, 7);
                Line(dash, 4, 14, 24, 14);
                Line(micro, 4, 21, 9, 21);
                Dot(14, 21, 1.05);
                Line(micro, 19, 21, 24, 21);
                break;

            case "lineweight":
            case "line-weight":
                Line(micro, 4, 6, 24, 6);
                context.DrawLine(new Pen(quiet, Math.Clamp(1.65 * scale, 1, 2.1)), P(4, 13), P(24, 13));
                context.DrawLine(new Pen(accent, Math.Clamp(2.7 * scale, 1.5, 3.6)), P(4, 21), P(24, 21));
                Dot(24, 21, 1.15);
                break;

            case "palettes":
                Rect(pen, 4, 5, 20, 18);
                Line(fine, 4, 10, 24, 10);
                Line(fine, 11, 10, 11, 23);
                context.DrawRectangle(wash, null, R(6, 12, 3, 3));
                context.DrawRectangle(wash, null, R(6, 17, 3, 3));
                Line(fine, 14, 13.5, 21, 13.5);
                Line(fine, 14, 18.5, 21, 18.5);
                break;

            case "paste":
                context.DrawRectangle(wash, pen, R(6, 3, 16, 22.5));
                context.DrawRectangle(blueWash, null, R(10, 0, 8, 7));
                Rect(fine, 9, 11, 10, 9);
                if (detailed)
                {
                    Line(fine, 11, 14, 17, 14);
                    Line(fine, 11, 17, 16, 17);
                }
                break;

            case "cut":
                context.DrawEllipse(surface, pen, P(8, 20), 3.6 * scale, 3.6 * scale);
                context.DrawEllipse(surface, pen, P(20, 20), 3.6 * scale, 3.6 * scale);
                Line(pen, 10.5, 17.5, 22, 5);
                Line(pen, 17.5, 17.5, 6, 5);
                Dot(14, 14, 1.35);
                if (detailed)
                {
                    Line(micro, 6, 5, 9, 5.8);
                    Line(micro, 22, 5, 19, 5.8);
                }
                break;

            case "zoom":
                Ellipse(pen, 12, 12, 7, 7);
                Line(bluePen, 17, 17, 24, 24);
                if (detailed)
                {
                    Line(fine, 8, 12, 16, 12);
                    Line(fine, 12, 8, 12, 16);
                }
                break;

            case "pan":
                Shape(wash, pen, 7, 14, 8, 7, 10, 7, 11, 13, 11, 5, 13, 4,
                    14, 12, 15, 5, 17, 5, 17, 13, 19, 8, 21, 9, 20, 19,
                    17, 24, 10, 23, 6, 18);
                break;

            case "view":
                Curve(pen, 3, 14, 8, 6, 20, 6, 25, 14);
                Curve(pen, 3, 14, 8, 22, 20, 22, 25, 14);
                Ellipse(fine, 14, 14, 4, 4);
                Dot(14, 14, 1.5);
                break;

            case "similar":
                Shape(wash, pen, 4, 11, 10, 5, 16, 11, 10, 17);
                Shape(null, fine, 12, 17, 18, 11, 24, 17, 18, 23);
                Line(pen, 18, 6, 24, 6);
                Line(pen, 21, 3, 21, 9);
                break;

            case "filter":
                Shape(wash, pen, 4, 5, 24, 5, 17, 14, 17, 22, 11, 25, 11, 14);
                Line(fine, 8, 9, 20, 9);
                break;

            case "new":
                Shape(surface, pen, 6, 4, 17, 4, 22, 9, 22, 24, 6, 24);
                Shape(blueWash, bluePen, 17, 4, 22, 9, 17, 9);
                Line(bluePen, 10, 17, 18, 17);
                Line(bluePen, 14, 13, 14, 21);
                break;

            case "open":
                Shape(wash, fine, 4, 8, 11, 8, 13, 11, 24, 11, 21, 23, 4, 23);
                Shape(surface, pen, 4, 13, 25, 13, 21, 23, 4, 23);
                Line(bluePen, 7, 16, 21, 16);
                break;

            case "save":
                context.DrawRectangle(surface, pen, R(5, 4, 18, 20));
                context.DrawRectangle(null, fine, R(9, 4, 10, 7));
                context.DrawRectangle(blueWash, bluePen, R(9, 16, 10, 8));
                Line(bluePen, 16, 6, 16, 10);
                break;

            case "saveas":
            case "save-as":
                context.DrawRectangle(surface, pen, R(5, 4, 18, 20));
                context.DrawRectangle(null, fine, R(9, 4, 10, 7));
                context.DrawRectangle(blueWash, bluePen, R(9, 16, 10, 8));
                Line(bluePen, 15, 21, 23, 13);
                Shape(blue, null, 22.5, 12.5, 25, 15, 23, 17);
                break;

            case "saveall":
            case "save-all":
                context.DrawRectangle(surface, fine, R(3, 6, 16, 18));
                context.DrawRectangle(surface, pen, R(8, 3, 16, 18));
                context.DrawRectangle(null, fine, R(12, 3, 8, 6));
                context.DrawRectangle(blueWash, bluePen, R(12, 14, 8, 7));
                break;

            case "undo":
                Curve(pen, 23, 21, 23, 10, 13, 7, 6, 13);
                Shape(blue, null, 5, 13, 11, 8, 11, 17);
                break;

            case "redo":
                Curve(pen, 5, 21, 5, 10, 15, 7, 22, 13);
                Shape(blue, null, 23, 13, 17, 8, 17, 17);
                break;

            case "print":
                context.DrawRectangle(surface, pen, R(4, 10, 20, 10));
                Rect(fine, 8, 4, 12, 7);
                context.DrawRectangle(blueWash, bluePen, R(8, 16, 12, 8));
                Dot(20.5, 13.5, 1);
                if (detailed)
                {
                    Line(fine, 10, 19, 18, 19);
                    Line(fine, 10, 21.5, 17, 21.5);
                }
                break;

            case "grid":
                for (var x = 6d; x <= 22; x += 5.33)
                {
                    for (var y = 6d; y <= 22; y += 5.33)
                    {
                        context.DrawEllipse(accent, null, P(x, y), 0.8 * scale, 0.8 * scale);
                    }
                }
                break;

            case "snap":
                Rect(fine, 5, 5, 18, 18);
                Line(micro, 11, 5, 11, 23);
                Line(micro, 17, 5, 17, 23);
                Line(micro, 5, 11, 23, 11);
                Line(micro, 5, 17, 23, 17);
                Dot(17, 11, 1.3);
                break;

            case "ortho":
                Line(pen, 6, 5, 6, 22);
                Line(pen, 6, 22, 23, 22);
                Rect(bluePen, 9, 16, 4, 4);
                break;

            case "polar":
                Ellipse(fine, 14, 14, 10, 10);
                Line(pen, 14, 14, 22, 6);
                Shape(blue, null, 22, 6, 19, 7, 21, 10);
                Dot(14, 14, 1.1);
                break;

            case "osnap":
                Shape(null, pen, 14, 3, 25, 14, 14, 25, 3, 14);
                context.DrawRectangle(blueWash, bluePen, R(11, 11, 6, 6));
                break;

            case "otrack":
                Line(dash, 5, 23, 22, 6);
                Shape(blue, null, 22, 6, 18, 7, 21, 11);
                Line(fine, 5, 17, 5, 23);
                Line(fine, 5, 23, 11, 23);
                break;

            case "dynamic":
                Rect(fine, 4, 7, 20, 14);
                Line(bluePen, 8, 12, 15, 12);
                Line(bluePen, 8, 16, 19, 16);
                Line(pen, 21, 10, 21, 18);
                break;

            case "ducs":
                Line(pen, 14, 15, 24, 20);
                Line(fine, 14, 15, 5, 21);
                Line(bluePen, 14, 15, 14, 4);
                Dot(14, 15, 1.2);
                break;

            case "selection":
                Rect(fine, 5, 5, 12, 12);
                Rect(pen, 11, 11, 12, 12);
                Box(11, 11, 2.4);
                Box(23, 23, 2.4);
                break;

            case "cursor":
                Shape(accent, bluePen, 5, 3, 22, 14, 15, 16, 19, 24, 15, 26, 11, 18, 6, 22);
                break;

            case "cloud":
                Curve(pen, 5, 20, 2, 18, 4, 13, 9, 13);
                Curve(pen, 9, 13, 10, 7, 19, 7, 20, 14);
                Curve(pen, 20, 14, 26, 13, 27, 21, 21, 22);
                Line(pen, 5, 20, 10, 22);
                Line(pen, 10, 22, 21, 22);
                Line(bluePen, 14, 17, 14, 25);
                Poly(bluePen, 11, 22, 14, 25, 17, 22);
                break;

            case "bell":
                Curve(pen, 7, 20, 10, 17, 8, 8, 14, 7);
                Curve(pen, 14, 7, 20, 8, 18, 17, 21, 20);
                Line(pen, 7, 20, 21, 20);
                Curve(bluePen, 11, 23, 12, 26, 16, 26, 17, 23);
                break;

            case "annotation":
                Poly(pen, 5, 23, 14, 4, 23, 23);
                Line(bluePen, 9, 16, 19, 16);
                break;

            case "paper":
                Shape(surface, pen, 6, 4, 18, 4, 23, 9, 23, 24, 6, 24);
                Shape(blueWash, bluePen, 18, 4, 23, 9, 18, 9);
                Line(fine, 10, 14, 19, 14);
                Line(fine, 10, 18, 19, 18);
                break;

            case "fullscreen":
                Poly(pen, 4, 11, 4, 4, 11, 4);
                Poly(pen, 17, 4, 24, 4, 24, 11);
                Poly(pen, 24, 17, 24, 24, 17, 24);
                Poly(pen, 11, 24, 4, 24, 4, 17);
                break;

            case "more":
                context.DrawEllipse(accent, null, P(6, 14), 1.7 * scale, 1.7 * scale);
                context.DrawEllipse(accent, null, P(14, 14), 1.7 * scale, 1.7 * scale);
                context.DrawEllipse(accent, null, P(22, 14), 1.7 * scale, 1.7 * scale);
                break;

            case "settings":
            case "gearsmall":
            case "gear-small":
                Ellipse(pen, 14, 14, 7, 7);
                context.DrawEllipse(blueWash, bluePen, P(14, 14), 2.6 * scale, 2.6 * scale);
                Line(pen, 14, 3, 14, 7);
                Line(pen, 14, 21, 14, 25);
                Line(pen, 3, 14, 7, 14);
                Line(pen, 21, 14, 25, 14);
                Line(pen, 6.2, 6.2, 9, 9);
                Line(pen, 19, 19, 21.8, 21.8);
                Line(pen, 21.8, 6.2, 19, 9);
                Line(pen, 9, 19, 6.2, 21.8);
                break;

            // ---------------------------------------------------------------
            // Distinctive vector icons replace ambiguous glyphs and duplicate artwork.
            // Same visual language as the base set: outline in 'pen', detail in 'fine'/'micro',
            // accents and arrows in 'blue'/'bluePen', and fills in wash/surface/blueWash.
            // ---------------------------------------------------------------

            // --- Annotation / dimensions ---
            case "multiline":
            case "multi-line":
                Line(pen, 5, 7, 22, 7);
                Line(pen, 5, 12, 22, 12);
                Line(pen, 5, 17, 18, 17);
                Line(bluePen, 5, 22, 13, 22);
                break;

            case "dimlinear":
            case "dim-linear":
                Rect(fine, 5, 15, 18, 7);
                Line(fine, 5, 10, 5, 15);
                Line(fine, 23, 10, 23, 15);
                Line(pen, 5, 10, 23, 10);
                Shape(blue, null, 5, 10, 9, 7.5, 9, 12.5);
                Shape(blue, null, 23, 10, 19, 7.5, 19, 12.5);
                break;

            case "angular":
            case "angular-dim":
                Line(pen, 6, 22, 24, 22);
                Line(pen, 6, 22, 22, 8);
                Curve(bluePen, 15, 22, 15, 18, 14, 15, 11.5, 13);
                Dot(6, 22, 1.15);
                break;

            case "radius":
            case "radius-dim":
                Ellipse(fine, 12, 14, 8, 8);
                Line(pen, 12, 14, 20, 8);
                Shape(blue, null, 20, 8, 15.5, 8.5, 17.5, 12);
                Dot(12, 14, 1.1);
                break;

            case "centermark":
            case "center-mark":
                Ellipse(micro, 14, 14, 7, 7);
                Line(pen, 14, 5, 14, 11);
                Line(pen, 14, 17, 14, 23);
                Line(pen, 5, 14, 11, 14);
                Line(pen, 17, 14, 23, 14);
                Dot(14, 14, 1.15);
                break;

            case "annoscale":
            case "anno-scale":
                Poly(pen, 4, 22, 10, 6, 16, 22);
                Line(pen, 7, 16, 13, 16);
                Poly(bluePen, 16, 23, 19.5, 14, 23, 23);
                Line(bluePen, 17.5, 19, 21.5, 19);
                break;

            case "multileader":
            case "multi-leader":
                Line(pen, 14, 7, 24, 7);
                Line(fine, 15, 11, 22, 11);
                Line(pen, 14, 8, 5, 20);
                Line(pen, 14, 8, 11, 22);
                Shape(blue, null, 5, 20, 9, 18, 7, 15);
                Shape(blue, null, 11, 22, 13, 18, 9, 19);
                break;

            case "revcloud":
            case "rev-cloud":
                Curve(pen, 5, 12, 5, 8, 9, 7, 11, 9);
                Curve(pen, 11, 9, 12, 6, 17, 6, 18, 9);
                Curve(pen, 18, 9, 21, 8, 24, 11, 22, 13);
                Curve(pen, 22, 13, 24, 16, 21, 19, 18, 18);
                Curve(pen, 18, 18, 17, 21, 12, 21, 11, 18);
                Curve(pen, 11, 18, 8, 20, 4, 17, 6, 14);
                Curve(pen, 6, 14, 4, 13, 4, 12, 5, 12);
                break;

            case "comment":
                context.DrawRectangle(wash, pen, R(4, 5, 20, 13));
                Shape(wash, pen, 8, 17, 8, 23, 14, 18);
                Line(fine, 8, 10, 20, 10);
                Line(fine, 8, 13.5, 16, 13.5);
                break;

            // --- Insert / references ---
            case "attachdwg":
            case "attach-dwg":
                Shape(surface, pen, 6, 4, 17, 4, 22, 9, 22, 21, 6, 21);
                Shape(blueWash, bluePen, 17, 4, 22, 9, 17, 9);
                Line(fine, 9, 12, 18, 12);
                Line(fine, 9, 15, 15, 15);
                Ellipse(bluePen, 12, 23, 2.2, 2.2);
                Ellipse(bluePen, 17, 23, 2.2, 2.2);
                Line(bluePen, 13.6, 23, 15.4, 23);
                break;

            case "image":
                Rect(pen, 4, 6, 20, 16);
                context.DrawEllipse(blue, null, P(9, 11), 1.6 * scale, 1.6 * scale);
                Poly(fine, 5, 21, 11, 14, 15, 18, 19, 12, 23, 20);
                break;

            case "pdf":
                Shape(surface, pen, 6, 3, 17, 3, 22, 8, 22, 25, 6, 25);
                Shape(blueWash, bluePen, 17, 3, 22, 8, 17, 8);
                Line(fine, 9, 12, 19, 12);
                Line(fine, 9, 15, 16, 15);
                context.DrawRectangle(warm, null, R(6, 19, 16, 5));
                break;

            case "pdfimport":
            case "pdf-import":
                Shape(surface, pen, 6, 3, 17, 3, 22, 8, 22, 25, 6, 25);
                context.DrawRectangle(warm, null, R(6, 20, 16, 5));
                Line(bluePen, 14, 6, 14, 15);
                Shape(blue, null, 14, 16, 10.5, 11, 17.5, 11);
                break;

            case "xref":
                Rect(pen, 3, 7, 9, 14);
                Rect(fine, 16, 7, 9, 14);
                Ellipse(bluePen, 12.5, 14, 2, 2);
                Ellipse(bluePen, 15.5, 14, 2, 2);
                Line(bluePen, 12, 14, 16, 14);
                break;

            case "location":
            case "map-pin":
                context.DrawEllipse(surface, pen, P(14, 11), 6.5 * scale, 6.5 * scale);
                Shape(wash, pen, 8.5, 15, 14, 25, 19.5, 15);
                context.DrawEllipse(blueWash, bluePen, P(14, 11), 2.6 * scale, 2.6 * scale);
                break;

            case "pin":
                context.DrawEllipse(surface, pen, P(14, 10), 5 * scale, 5 * scale);
                context.DrawEllipse(blueWash, bluePen, P(14, 10), 2 * scale, 2 * scale);
                Line(pen, 14, 15, 14, 24);
                break;

            case "autohide":
            case "auto-hide":
                context.DrawEllipse(surface, pen, P(9, 14), 5 * scale, 5 * scale);
                context.DrawEllipse(blueWash, bluePen, P(9, 14), 2 * scale, 2 * scale);
                Line(pen, 14, 14, 24, 14);
                break;

            // --- Views ---
            case "viewtop":
            case "view-top":
                Shape(blueWash, bluePen, 14, 4, 24, 10, 14, 16, 4, 10);
                Shape(wash, pen, 4, 10, 14, 16, 14, 25, 4, 19);
                Shape(surface, pen, 24, 10, 14, 16, 14, 25, 24, 19);
                break;

            case "viewfront":
            case "view-front":
                Shape(surface, fine, 14, 4, 24, 10, 14, 16, 4, 10);
                Shape(blueWash, bluePen, 4, 10, 14, 16, 14, 25, 4, 19);
                Shape(wash, pen, 24, 10, 14, 16, 14, 25, 24, 19);
                break;

            case "viewiso":
            case "view-iso":
                Shape(surface, pen, 14, 4, 24, 10, 14, 16, 4, 10);
                Shape(wash, pen, 4, 10, 14, 16, 14, 25, 4, 19);
                Shape(wash, pen, 24, 10, 14, 16, 14, 25, 24, 19);
                Dot(14, 16, 1.1);
                break;

            case "orbit":
                Ellipse(pen, 14, 14, 6, 6);
                Ellipse(bluePen, 14, 14, 11, 4);
                Shape(blue, null, 24, 15, 21, 12.5, 22.5, 16.5);
                Dot(14, 14, 1);
                break;

            case "zoomextents":
            case "zoom-extents":
                Rect(fine, 10, 10, 8, 8);
                Line(bluePen, 12, 12, 5, 5);
                Shape(blue, null, 5, 5, 9, 6, 6, 9);
                Line(bluePen, 16, 12, 23, 5);
                Shape(blue, null, 23, 5, 19, 6, 22, 9);
                Line(bluePen, 12, 16, 5, 23);
                Shape(blue, null, 5, 23, 6, 19, 9, 22);
                Line(bluePen, 16, 16, 23, 23);
                Shape(blue, null, 23, 23, 19, 22, 22, 19);
                break;

            case "realistic":
                context.DrawEllipse(wash, pen, P(14, 14), 9 * scale, 9 * scale);
                Curve(fine, 8, 7, 14, 10, 16, 18, 12, 22);
                context.DrawEllipse(accent, null, P(10, 10), 1.5 * scale, 1.5 * scale);
                break;

            // --- Measure / inspect ---
            case "area":
                Shape(blueWash, pen, 5, 8, 20, 5, 24, 16, 13, 24, 4, 17);
                Box(20, 5, 2.4);
                Box(13, 24, 2.4);
                Line(micro, 8, 12, 18, 10);
                Line(micro, 8, 16, 20, 14);
                break;

            case "calculator":
                context.DrawRectangle(surface, pen, R(6, 3, 16, 22));
                context.DrawRectangle(blueWash, bluePen, R(8, 5, 12, 5));
                Dot(10, 15, 1);
                Dot(14, 15, 1);
                Dot(18, 15, 1);
                Dot(10, 19, 1);
                Dot(14, 19, 1);
                Dot(18, 19, 1);
                break;

            case "massprops":
            case "mass-props":
                context.DrawRectangle(wash, pen, R(6, 7, 16, 14));
                Ellipse(bluePen, 14, 14, 3.5, 3.5);
                Line(bluePen, 14, 10.5, 14, 17.5);
                Line(bluePen, 10.5, 14, 17.5, 14);
                break;

            case "pointid":
            case "point-id":
                Line(fine, 14, 4, 14, 24);
                Line(fine, 4, 14, 24, 14);
                Ellipse(pen, 14, 14, 5, 5);
                Dot(14, 14, 1.6);
                break;

            case "search":
                Ellipse(pen, 12, 12, 6, 6);
                Line(pen, 16.5, 16.5, 23, 23);
                break;

            case "duplicates":
                context.DrawRectangle(surface, fine, R(5, 5, 12, 12));
                context.DrawRectangle(wash, pen, R(10, 10, 12, 12));
                Line(bluePen, 13, 15, 19, 15);
                Line(bluePen, 13, 18, 19, 18);
                break;

            case "extractdata":
            case "extract-data":
                context.DrawRectangle(wash, pen, R(4, 5, 14, 16));
                Line(fine, 4, 11, 18, 11);
                Line(fine, 11, 5, 11, 21);
                Line(bluePen, 18, 14, 24, 14);
                Shape(blue, null, 24, 14, 20.5, 11.5, 20.5, 16.5);
                break;

            // --- Cloud / collaboration ---
            case "cloudoff":
            case "cloud-off":
                Curve(fine, 5, 20, 2, 18, 4, 13, 9, 13);
                Curve(fine, 9, 13, 10, 7, 19, 7, 20, 14);
                Curve(fine, 20, 14, 26, 13, 27, 21, 21, 22);
                Line(fine, 5, 20, 21, 22);
                Line(bluePen, 5, 6, 23, 24);
                break;

            case "sharelink":
            case "share-link":
                context.DrawEllipse(surface, pen, P(7, 14), 3 * scale, 3 * scale);
                context.DrawEllipse(surface, pen, P(20, 7), 3 * scale, 3 * scale);
                context.DrawEllipse(surface, pen, P(20, 21), 3 * scale, 3 * scale);
                Line(bluePen, 9.5, 13, 17.5, 8);
                Line(bluePen, 9.5, 15, 17.5, 20);
                Dot(7, 14, 1.1);
                Dot(20, 7, 1.1);
                Dot(20, 21, 1.1);
                break;

            case "cloudsync":
            case "cloud-sync":
                Curve(fine, 5, 17, 2, 15, 4, 10, 9, 10);
                Curve(fine, 9, 10, 10, 4, 19, 4, 20, 11);
                Curve(fine, 20, 11, 26, 10, 27, 18, 21, 19);
                Line(fine, 5, 17, 21, 19);
                Curve(bluePen, 9, 23, 9, 26, 15, 26, 16, 23);
                Shape(blue, null, 16, 23, 13.5, 23.5, 15.5, 25.5);
                Dot(9, 22.5, 0.9);
                break;

            case "sync":
                Curve(pen, 6, 11, 8, 5, 18, 4, 22, 9);
                Shape(blue, null, 22, 9, 17.5, 8, 20, 12);
                Curve(pen, 22, 17, 20, 23, 10, 24, 6, 19);
                Shape(blue, null, 6, 19, 10.5, 20, 8, 16);
                break;

            case "history":
            case "historyclock":
                Ellipse(pen, 15, 15, 8, 8);
                Line(pen, 15, 15, 15, 10);
                Line(pen, 15, 15, 19, 16);
                Curve(bluePen, 15, 5, 8, 5, 5, 9, 6, 13);
                Shape(blue, null, 6, 13, 3.5, 9.5, 8.5, 10);
                break;

            case "restore":
                context.DrawRectangle(wash, fine, R(10, 10, 9, 9));
                Curve(pen, 14, 5, 7, 5, 4, 10, 6, 15);
                Shape(blue, null, 6, 15, 3, 11, 8.5, 11.5);
                break;

            case "version":
                Shape(surface, pen, 5, 8, 15, 8, 24, 14, 15, 20, 5, 20);
                context.DrawEllipse(blue, null, P(9, 14), 1.6 * scale, 1.6 * scale);
                break;

            // --- Smart ---
            case "assistant":
                context.DrawRectangle(wash, pen, R(4, 5, 20, 13));
                Shape(wash, pen, 8, 17, 8, 23, 14, 18);
                Line(bluePen, 14, 7, 14, 16);
                Line(bluePen, 9.5, 11.5, 18.5, 11.5);
                Line(bluePen, 11, 8.5, 17, 14.5);
                Line(bluePen, 17, 8.5, 11, 14.5);
                break;

            case "suggest":
                Ellipse(pen, 14, 11, 6, 6);
                Line(pen, 11, 17, 11, 20);
                Line(pen, 17, 17, 17, 20);
                Line(fine, 11, 21, 17, 21);
                Line(fine, 12, 23, 16, 23);
                Line(bluePen, 14, 8, 14, 14);
                break;

            case "compare":
                context.DrawRectangle(surface, pen, R(4, 6, 8, 16));
                context.DrawRectangle(wash, pen, R(16, 6, 8, 16));
                Line(bluePen, 6, 14, 10, 14);
                Line(bluePen, 20, 11, 20, 17);
                Line(bluePen, 18, 14, 22, 14);
                break;

            case "opening":
                context.DrawRectangle(wash, pen, R(4, 12, 6, 6));
                context.DrawRectangle(wash, pen, R(18, 12, 6, 6));
                Line(bluePen, 10, 18, 10, 12);
                Curve(bluePen, 10, 12, 14, 12, 17, 14, 18, 18);
                break;

            case "cleandrawing":
            case "clean-drawing":
                Line(pen, 21, 4, 13, 14);
                Shape(wash, pen, 9, 13, 15, 11, 19, 17, 12, 20);
                Line(fine, 10, 18, 9, 23);
                Line(fine, 13, 19, 12.5, 24);
                Line(fine, 16, 19, 16, 24);
                Dot(6, 8, 1);
                Dot(8, 5, 0.9);
                break;

            case "purge":
                Line(pen, 5, 8, 23, 8);
                Line(pen, 11, 5, 17, 5);
                Line(pen, 11, 5, 11, 8);
                Line(pen, 17, 5, 17, 8);
                Shape(wash, pen, 7, 8, 21, 8, 19, 24, 9, 24);
                Line(fine, 12, 11, 12, 21);
                Line(fine, 16, 11, 16, 21);
                break;

            // --- Account / permissions / settings ---
            case "account":
                context.DrawEllipse(wash, pen, P(14, 9), 4.2 * scale, 4.2 * scale);
                Shape(wash, pen, 5, 24, 6, 18, 22, 18, 23, 24);
                break;

            case "permissions":
                Shape(blueWash, pen, 14, 3, 23, 7, 22, 15, 14, 25, 6, 15, 5, 7);
                Poly(bluePen, 10, 13, 13, 17, 19, 9);
                break;

            case "sliders":
                Line(fine, 5, 8, 23, 8);
                Line(fine, 5, 14, 23, 14);
                Line(fine, 5, 20, 23, 20);
                context.DrawEllipse(blueWash, bluePen, P(11, 8), 2 * scale, 2 * scale);
                context.DrawEllipse(blueWash, bluePen, P(17, 14), 2 * scale, 2 * scale);
                context.DrawEllipse(blueWash, bluePen, P(9, 20), 2 * scale, 2 * scale);
                break;

            case "checkstandards":
            case "check-standards":
                context.DrawRectangle(wash, pen, R(6, 5, 16, 20));
                context.DrawRectangle(surface, pen, R(10, 3, 8, 4));
                Poly(bluePen, 9, 15, 13, 20, 20, 10);
                break;

            case "audit":
                Shape(surface, fine, 5, 4, 16, 4, 20, 8, 20, 22, 5, 22);
                Line(fine, 8, 9, 17, 9);
                Line(fine, 8, 12, 15, 12);
                Ellipse(pen, 14, 16, 4, 4);
                Line(pen, 17, 19, 22, 24);
                break;

            // --- Geoservices / maps ---
            case "mapbase":
            case "map-base":
                Shape(surface, pen, 4, 8, 10, 6, 17, 8, 24, 6, 24, 20, 17, 22, 10, 20, 4, 22);
                Line(fine, 10, 6, 10, 20);
                Line(fine, 17, 8, 17, 22);
                Curve(bluePen, 7, 18, 10, 13, 15, 15, 21, 10);
                Dot(21, 10, 1.2);
                break;

            case "satellite":
                Curve(fine, 3, 24, 9, 20, 19, 20, 25, 24);
                context.DrawRectangle(wash, pen, R(12, 9, 4, 5));
                context.DrawRectangle(blueWash, bluePen, R(5, 10, 5, 3));
                context.DrawRectangle(blueWash, bluePen, R(18, 10, 5, 3));
                Curve(bluePen, 15, 8, 18, 5, 20, 6, 21, 4);
                break;

            case "markpoint":
            case "mark-point":
                Line(pen, 8, 4, 8, 24);
                Shape(blueWash, bluePen, 8, 5, 20, 8.5, 8, 12);
                Dot(8, 24, 1.5);
                Line(fine, 4, 24, 12, 24);
                break;

            case "units":
                context.DrawRectangle(wash, pen, R(4, 11, 20, 7));
                Line(fine, 8, 11, 8, 15);
                Line(fine, 12, 11, 12, 14);
                Line(fine, 16, 11, 16, 15);
                Line(fine, 20, 11, 20, 14);
                break;

            case "exportgis":
            case "export-gis":
                Ellipse(pen, 11, 13, 7, 7);
                Ellipse(fine, 11, 13, 3, 7);
                Line(fine, 4, 13, 18, 13);
                Line(bluePen, 17, 20, 24, 25);
                Shape(blue, null, 24, 25, 19.5, 24, 22, 20.5);
                break;

            // --- Export formats ---
            case "formatdwg":
            case "format-dwg":
                Shape(surface, pen, 6, 4, 17, 4, 22, 9, 22, 25, 6, 25);
                Shape(blueWash, bluePen, 17, 4, 22, 9, 17, 9);
                Poly(bluePen, 9, 20, 12, 15, 15, 19, 19, 13);
                Box(9, 20, 2);
                Box(19, 13, 2);
                break;

            case "formatdxf":
            case "format-dxf":
                Shape(surface, pen, 6, 4, 17, 4, 22, 9, 22, 25, 6, 25);
                Shape(blueWash, bluePen, 17, 4, 22, 9, 17, 9);
                Line(bluePen, 10, 14, 18, 23);
                Line(bluePen, 18, 14, 10, 23);
                break;

            case "formatdwf":
            case "format-dwf":
                Shape(surface, pen, 6, 4, 17, 4, 22, 9, 22, 25, 6, 25);
                Shape(blueWash, bluePen, 17, 4, 22, 9, 17, 9);
                context.DrawEllipse(blue, null, P(10, 20), 1.3 * scale, 1.3 * scale);
                context.DrawEllipse(blue, null, P(18, 14), 1.3 * scale, 1.3 * scale);
                context.DrawEllipse(blue, null, P(18, 22), 1.3 * scale, 1.3 * scale);
                Line(bluePen, 11, 19, 17, 15);
                Line(bluePen, 11, 20.5, 17, 21.5);
                break;

            case "mesh3d":
            case "mesh-3d":
                Shape(surface, pen, 14, 4, 24, 10, 24, 20, 14, 26, 4, 20, 4, 10);
                Line(pen, 14, 4, 14, 26);
                Line(pen, 4, 10, 24, 20);
                Line(pen, 24, 10, 4, 20);
                Line(fine, 14, 15, 24, 10);
                Line(fine, 14, 15, 4, 10);
                break;

            case "publish":
                context.DrawRectangle(surface, fine, R(8, 8, 13, 15));
                context.DrawRectangle(wash, pen, R(4, 11, 13, 14));
                Line(bluePen, 22, 20, 22, 10);
                Shape(blue, null, 22, 8, 18.5, 13, 25.5, 13);
                break;

            // --- Layers (mini-button glyphs converted to vectors for XAML wiring) ---
            case "layerzero":
            case "layer-zero":
                Shape(wash, pen, 4, 9, 14, 4, 24, 9, 14, 14);
                Poly(fine, 4, 14, 14, 19, 24, 14);
                Poly(fine, 4, 19, 14, 24, 24, 19);
                context.DrawEllipse(null, bluePen, P(14, 9), 2.2 * scale, 3 * scale);
                break;

            case "visibility":
                context.DrawEllipse(wash, pen, P(14, 14), 4.5 * scale, 4.5 * scale);
                Line(bluePen, 14, 3, 14, 6);
                Line(bluePen, 14, 22, 14, 25);
                Line(bluePen, 3, 14, 6, 14);
                Line(bluePen, 22, 14, 25, 14);
                Line(bluePen, 6.5, 6.5, 8.5, 8.5);
                Line(bluePen, 19.5, 19.5, 21.5, 21.5);
                Line(bluePen, 21.5, 6.5, 19.5, 8.5);
                Line(bluePen, 8.5, 19.5, 6.5, 21.5);
                break;

            case "lock":
                Curve(pen, 9, 13, 9, 6, 19, 6, 19, 13);
                context.DrawRectangle(wash, pen, R(6, 13, 16, 11));
                Dot(14, 17.5, 1.2);
                Line(bluePen, 14, 18, 14, 21);
                break;

            case "isolate":
                Poly(micro, 4, 8, 14, 3, 24, 8);
                Shape(blueWash, bluePen, 4, 14, 14, 9, 24, 14, 14, 19);
                Poly(micro, 4, 20, 14, 25, 24, 20);
                break;

            case "layerstates":
            case "layer-states":
                Shape(wash, pen, 4, 10, 14, 5, 24, 10, 14, 15);
                Poly(fine, 4, 15, 14, 20, 24, 15);
                Poly(fine, 4, 20, 14, 25, 24, 20);
                Shape(blueWash, bluePen, 18, 3, 24, 3, 24, 12, 21, 9.5, 18, 12);
                break;

            case "layerlist":
            case "layer-list":
                Box(7, 7, 3.4);
                Line(fine, 11, 7, 23, 7);
                context.DrawRectangle(quiet, null, R(5.3, 12.3, 3.4, 3.4));
                Line(fine, 11, 14, 23, 14);
                Box(7, 21, 3.4);
                Line(fine, 11, 21, 23, 21);
                break;

            case "layertranslate":
            case "layer-translate":
                Shape(wash, pen, 3, 9, 8, 6.5, 13, 9, 8, 11.5);
                Shape(surface, pen, 15, 18, 20, 15.5, 25, 18, 20, 20.5);
                Line(bluePen, 10, 12, 18, 17);
                Shape(blue, null, 18, 17, 13.5, 15.5, 15, 19.5);
                break;

            case "merge":
                Line(pen, 5, 6, 13, 14);
                Line(pen, 5, 22, 13, 14);
                Line(pen, 13, 14, 23, 14);
                Shape(blue, null, 23, 14, 19, 11.5, 19, 16.5);
                break;

            // --- Chrome glyphs (chevrons, +/-, close, help, window) ---
            case "chevrondown":
            case "chevron-down":
                Poly(pen, 7, 11, 14, 18, 21, 11);
                break;

            case "chevronup":
            case "chevron-up":
                Poly(pen, 7, 17, 14, 10, 21, 17);
                break;

            case "chevronright":
            case "chevron-right":
                Poly(pen, 11, 7, 18, 14, 11, 21);
                break;

            // Layer plot/freeze markers: a snowflake reads as frozen; a warm sun reads
            // as thawed or enabled for plotting.
            case "freeze":
            case "snowflake":
                Line(bluePen, 14, 5, 14, 23);
                Line(bluePen, 6.5, 9.5, 21.5, 18.5);
                Line(bluePen, 6.5, 18.5, 21.5, 9.5);
                Line(micro, 14, 5, 11.5, 7.5); Line(micro, 14, 5, 16.5, 7.5);
                Line(micro, 14, 23, 11.5, 20.5); Line(micro, 14, 23, 16.5, 20.5);
                break;

            case "sun":
            case "thaw":
            {
                var warmPen = new Pen(warm, Math.Clamp(1.4 * scale, 0.95, 1.65));
                Ellipse(warmPen, 14, 14, 4.6, 4.6);
                Line(warmPen, 14, 3.5, 14, 7); Line(warmPen, 14, 21, 14, 24.5);
                Line(warmPen, 3.5, 14, 7, 14); Line(warmPen, 21, 14, 24.5, 14);
                Line(warmPen, 6.5, 6.5, 9, 9); Line(warmPen, 19, 19, 21.5, 21.5);
                Line(warmPen, 6.5, 21.5, 9, 19); Line(warmPen, 19, 9, 21.5, 6.5);
                break;
            }

            case "close":
                Line(pen, 7, 7, 21, 21);
                Line(pen, 21, 7, 7, 21);
                break;

            case "plus":
                Line(pen, 14, 6, 14, 22);
                Line(pen, 6, 14, 22, 14);
                break;

            case "minus":
                Line(pen, 6, 14, 22, 14);
                break;

            case "help":
                Ellipse(micro, 14, 14, 9.5, 9.5);
                Curve(pen, 10.5, 11, 11, 7, 17, 7, 17.5, 11);
                Curve(pen, 17.5, 11, 17.5, 14, 14, 14.5, 14, 18);
                Dot(14, 21.5, 1.2);
                break;

            case "check":
                Poly(bluePen, 6, 14, 12, 20, 22, 7);
                break;

            case "windowminimize":
            case "window-minimize":
                Line(pen, 7, 20, 21, 20);
                break;

            case "windowmaximize":
            case "window-maximize":
                Rect(pen, 7, 7, 14, 14);
                break;

            case "windowrestore":
            case "window-restore":
                Rect(pen, 5, 9, 12, 12);
                Poly(fine, 9, 9, 9, 5, 21, 5, 21, 17, 17, 17);
                break;

            // Previous Layer and Next Layer shared the Layer icon. These variants
            // reuse the layer stack but add a directional arrow so adjacent mini buttons remain
            // distinguishable.
            case "layerprev":
            case "layer-prev":
                Shape(wash, pen, 4, 8, 14, 4, 24, 8, 14, 12);
                Poly(fine, 4, 12, 14, 16, 24, 12);
                Line(bluePen, 8, 21, 20, 21);
                Shape(blue, null, 8, 21, 12, 18.5, 12, 23.5);
                break;

            case "layernext":
            case "layer-next":
                Shape(wash, pen, 4, 8, 14, 4, 24, 8, 14, 12);
                Poly(fine, 4, 12, 14, 16, 24, 12);
                Line(bluePen, 8, 21, 20, 21);
                Shape(blue, null, 20, 21, 16, 18.5, 16, 23.5);
                break;

            // Viewport Lock used the generic Select icon (a selection frame).
            // A padlock over a viewport frame makes its meaning distinct.
            case "viewlock":
            case "view-lock":
                Rect(pen, 3, 5, 17, 12);
                Line(fine, 3, 9, 20, 9);
                Line(fine, 9, 9, 9, 17);
                Curve(bluePen, 16, 20, 16, 16.5, 22, 16.5, 22, 20);
                context.DrawRectangle(blueWash, bluePen, R(14.5, 19.5, 9, 6.5));
                Dot(19, 22.5, 1);
                break;

            // Every palette block used the same generic 'block' glyph.
            // Type-specific icons distinguish doors, windows, and furniture.
            case "door":
                Line(pen, 7, 4, 7, 22);
                Line(bluePen, 7, 22, 20, 22);
                Curve(fine, 20, 22, 20, 13, 15, 9, 7, 9);
                break;

            case "window":
                Rect(pen, 4, 7, 20, 14);
                Line(pen, 14, 7, 14, 21);
                Line(pen, 4, 14, 24, 14);
                break;

            case "furniture":
                context.DrawRectangle(wash, pen, R(8, 9, 12, 10));
                context.DrawRectangle(blueWash, bluePen, R(9, 5, 4, 3));
                context.DrawRectangle(blueWash, bluePen, R(15, 5, 4, 3));
                context.DrawRectangle(blueWash, bluePen, R(9, 20, 4, 3));
                context.DrawRectangle(blueWash, bluePen, R(15, 20, 4, 3));
                context.DrawRectangle(blueWash, bluePen, R(3, 11, 3, 6));
                context.DrawRectangle(blueWash, bluePen, R(22, 11, 3, 6));
                break;

            default:
                Ellipse(fine, 14, 14, 9, 9);
                Line(pen, 14, 7, 14, 16);
                Dot(14, 21, 1.2);
                break;
        }
    }
}
