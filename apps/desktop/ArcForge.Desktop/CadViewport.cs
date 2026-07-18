using System.Globalization;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Media;

namespace ArcForge.Desktop;

public readonly record struct CadLine(ulong EntityId, float X1, float Y1, float X2, float Y2)
{
    public double Length => Math.Sqrt(
        Math.Pow((double)X2 - X1, 2) +
        Math.Pow((double)Y2 - Y1, 2));
}

public readonly record struct CadEntityPath(
    ulong EntityId,
    ReadOnlyMemory<float> Vertices,
    float PolyWidth = 0,
    bool IsLine = true,
    bool IsMask = false,
    double? AnalyticLength = null)
{
    public int PointCount => Vertices.Length / 2;

    public double VisibleLength
    {
        get
        {
            var vertices = Vertices.Span;
            var length = 0.0;
            for (var index = 2; index < vertices.Length; index += 2)
            {
                var dx = (double)vertices[index] - vertices[index - 2];
                var dy = (double)vertices[index + 1] - vertices[index - 1];
                length += Math.Sqrt(dx * dx + dy * dy);
            }

            return length;
        }
    }

    public double Length => AnalyticLength ?? VisibleLength;

    public CadLine? AsLine => IsLine && Vertices.Length == 4
        ? new CadLine(EntityId, Vertices.Span[0], Vertices.Span[1], Vertices.Span[2], Vertices.Span[3])
        : null;
}

public readonly record struct CadMarker(ulong EntityId, float X, float Y);

public sealed class CadViewport : Control
{
    public static readonly StyledProperty<IBrush?> BackgroundProperty =
        Border.BackgroundProperty.AddOwner<CadViewport>();
    public static readonly StyledProperty<IBrush?> BorderBrushProperty =
        Border.BorderBrushProperty.AddOwner<CadViewport>();
    public static readonly StyledProperty<Thickness> BorderThicknessProperty =
        Border.BorderThicknessProperty.AddOwner<CadViewport>();
    public static readonly StyledProperty<bool> ShowGridProperty =
        AvaloniaProperty.Register<CadViewport, bool>(nameof(ShowGrid), true);
    public static readonly StyledProperty<bool> ShowUcsProperty =
        AvaloniaProperty.Register<CadViewport, bool>(nameof(ShowUcs), true);
    public static readonly StyledProperty<bool> ShowDimensionsProperty =
        AvaloniaProperty.Register<CadViewport, bool>(nameof(ShowDimensions));
    public static readonly StyledProperty<bool> UseHeavyLineweightProperty =
        AvaloniaProperty.Register<CadViewport, bool>(nameof(UseHeavyLineweight));

    private static readonly IBrush CanvasBrush = Brush.Parse("#111820");
    private static readonly Color Amber = Color.Parse("#FBBF24");
    private static readonly Color Cyan = Color.Parse("#22D3EE");
    private static readonly Typeface CadTypeface = new("Segoe UI");
    private static readonly Pen MinorGridPen = new(Brush.Parse("#1A232C"), 0.4);
    private static readonly Pen MajorGridPen = new(Brush.Parse("#26323D"), 0.6);
    private static readonly Pen LinePen = new(Brush.Parse("#D6DCE2"), 1.4);
    private static readonly Pen HeavyLinePen = new(Brush.Parse("#D6DCE2"), 2.6);
    private static readonly Pen SelectedPen = new(new SolidColorBrush(Cyan), 3);
    private static readonly Pen PreviewPen = new(new SolidColorBrush(Amber), 2);
    private static readonly Pen CrosshairPen = new(Brush.Parse("#9FA9B2"), 0.8);
    private static readonly IBrush UcsXBrush = Brush.Parse("#B86A72");
    private static readonly IBrush UcsYBrush = Brush.Parse("#6FA887");
    private static readonly Pen UcsXPen = new(UcsXBrush, 1.2);
    private static readonly Pen UcsYPen = new(UcsYBrush, 1.2);
    private static readonly Pen UcsOriginPen = new(Brush.Parse("#AAB3BB"), 1);
    private static readonly IBrush GripBrush = new SolidColorBrush(Cyan);
    private static readonly IBrush DimensionBrush = Brush.Parse("#8FB4CE");
    private CadEntityPath[] _entities = [];
    private CadMarker[] _markers = [];
    private CadLine[] _lines = [];
    private float[] _previewVertices = [];
    private ulong? _selectedEntityId;
    private Point _pointer;
    private bool _hasPointer;
    private float _cursorX;
    private float _cursorY;
    private bool _hasCursor;
    private bool _cursorSnapped;
    private double _zoom = 1;
    private double _panX;
    private double _panY;

    static CadViewport()
    {
        AffectsRender<CadViewport>(
            BackgroundProperty,
            BorderBrushProperty,
            BorderThicknessProperty,
            ShowGridProperty,
            ShowUcsProperty,
            ShowDimensionsProperty,
            UseHeavyLineweightProperty);
    }

    public CadViewport()
    {
        ClipToBounds = true;
        Focusable = true;
        PointerMoved += (_, eventArgs) =>
        {
            _pointer = eventArgs.GetPosition(this);
            _hasPointer = true;
            InvalidateVisual();
        };
        PointerExited += (_, _) =>
        {
            _hasPointer = false;
            InvalidateVisual();
        };
        AutomationProperties.SetName(this, "Área de dibujo");
    }

    public IBrush? Background
    {
        get => GetValue(BackgroundProperty);
        set => SetValue(BackgroundProperty, value);
    }

    public IBrush? BorderBrush
    {
        get => GetValue(BorderBrushProperty);
        set => SetValue(BorderBrushProperty, value);
    }

    public Thickness BorderThickness
    {
        get => GetValue(BorderThicknessProperty);
        set => SetValue(BorderThicknessProperty, value);
    }

    public bool ShowGrid
    {
        get => GetValue(ShowGridProperty);
        set => SetValue(ShowGridProperty, value);
    }

    public bool ShowUcs
    {
        get => GetValue(ShowUcsProperty);
        set => SetValue(ShowUcsProperty, value);
    }

    public bool ShowDimensions
    {
        get => GetValue(ShowDimensionsProperty);
        set => SetValue(ShowDimensionsProperty, value);
    }

    public bool UseHeavyLineweight
    {
        get => GetValue(UseHeavyLineweightProperty);
        set => SetValue(UseHeavyLineweightProperty, value);
    }

    public Control? Child => null;

    public ReadOnlyMemory<CadEntityPath> Entities => _entities;

    public ReadOnlyMemory<CadMarker> Markers => _markers;

    public ReadOnlyMemory<CadLine> Lines => _lines;

    public ReadOnlyMemory<float> PreviewVertices => _previewVertices;

    public ulong? SelectedEntityId => _selectedEntityId;

    public bool HasCursor => _hasCursor;

    public bool CursorSnapped => _cursorSnapped;

    public Point? CursorWorldPoint => _hasCursor ? new Point(_cursorX, _cursorY) : null;

    public double Zoom => _zoom;

    public Vector Pan => new(_panX, _panY);

    public Point WorldToViewport(double x, double y) =>
        new(x * _zoom + _panX, Bounds.Height - y * _zoom + _panY);

    public Point ViewportToWorld(Point point) =>
        new((point.X - _panX) / _zoom, (Bounds.Height + _panY - point.Y) / _zoom);

    public void ZoomAt(double factor, Point focus)
    {
        if (!double.IsFinite(factor) || factor <= 0 ||
            !double.IsFinite(focus.X) || !double.IsFinite(focus.Y))
        {
            throw new ArgumentOutOfRangeException(nameof(factor));
        }

        var world = ViewportToWorld(focus);
        var nextZoom = Math.Clamp(_zoom * factor, 0.25, 8);
        if (Math.Abs(nextZoom - _zoom) < double.Epsilon)
        {
            return;
        }

        _zoom = nextZoom;
        _panX = focus.X - world.X * _zoom;
        _panY = focus.Y - Bounds.Height + world.Y * _zoom;
        InvalidateVisual();
    }

    public void PanBy(Vector delta)
    {
        if (!double.IsFinite(delta.X) || !double.IsFinite(delta.Y))
        {
            throw new ArgumentOutOfRangeException(nameof(delta));
        }

        _panX += delta.X;
        _panY += delta.Y;
        InvalidateVisual();
    }

    public void FitToLines(double padding = 72)
        => FitToEntities(padding);

    public void FitToEntities(double padding = 72)
    {
        if (_entities.Length == 0 && _markers.Length == 0)
        {
            ResetView();
            return;
        }

        var minX = float.PositiveInfinity;
        var maxX = float.NegativeInfinity;
        var minY = float.PositiveInfinity;
        var maxY = float.NegativeInfinity;
        foreach (var entity in _entities)
        {
            var vertices = entity.Vertices.Span;
            var halfWidth = entity.PolyWidth / 2;
            for (var index = 0; index < vertices.Length; index += 2)
            {
                minX = Math.Min(minX, vertices[index] - halfWidth);
                maxX = Math.Max(maxX, vertices[index] + halfWidth);
                minY = Math.Min(minY, vertices[index + 1] - halfWidth);
                maxY = Math.Max(maxY, vertices[index + 1] + halfWidth);
            }
        }

        foreach (var marker in _markers)
        {
            minX = Math.Min(minX, marker.X);
            maxX = Math.Max(maxX, marker.X);
            minY = Math.Min(minY, marker.Y);
            maxY = Math.Max(maxY, marker.Y);
        }

        var availableWidth = Math.Max(1, Bounds.Width - padding * 2);
        var availableHeight = Math.Max(1, Bounds.Height - padding * 2);
        var worldWidth = Math.Max(1, maxX - minX);
        var worldHeight = Math.Max(1, maxY - minY);
        _zoom = Math.Clamp(Math.Min(availableWidth / worldWidth, availableHeight / worldHeight), 0.25, 8);

        var centerX = (minX + maxX) / 2.0;
        var centerY = (minY + maxY) / 2.0;
        _panX = Bounds.Width / 2.0 - centerX * _zoom;
        _panY = Bounds.Height / 2.0 - Bounds.Height + centerY * _zoom;
        InvalidateVisual();
    }

    public void ResetView()
    {
        _zoom = 1;
        _panX = 0;
        _panY = 0;
        InvalidateVisual();
    }

    public void SetLines(ReadOnlySpan<CadLine> lines)
    {
        var entities = new CadEntityPath[lines.Length];
        for (var index = 0; index < lines.Length; index++)
        {
            var line = lines[index];
            entities[index] = new CadEntityPath(
                line.EntityId,
                new float[] { line.X1, line.Y1, line.X2, line.Y2 }.AsMemory());
        }

        SetScene(entities, []);
    }

    public void SetEntities(ReadOnlySpan<CadEntityPath> entities) => SetScene(entities, []);

    public void SetScene(ReadOnlySpan<CadEntityPath> entities, ReadOnlySpan<CadMarker> markers)
    {
        var copy = new CadEntityPath[entities.Length];
        var markerCopy = new CadMarker[markers.Length];
        var lines = new List<CadLine>(entities.Length);
        var ids = new HashSet<ulong>();
        for (var index = 0; index < entities.Length; index++)
        {
            var entity = entities[index];
            var vertices = entity.Vertices.Span;
            if (entity.EntityId == 0 ||
                vertices.Length < 4 ||
                (vertices.Length & 1) != 0 ||
                !float.IsFinite(entity.PolyWidth) ||
                entity.PolyWidth < 0 ||
                entity.AnalyticLength is { } analyticLength &&
                    (!double.IsFinite(analyticLength) || analyticLength < 0) ||
                entity.IsLine && entity.IsMask ||
                entity.IsMask &&
                    (vertices.Length < 8 ||
                     vertices[0] != vertices[^2] ||
                     vertices[1] != vertices[^1]) ||
                !ids.Add(entity.EntityId))
            {
                throw new ArgumentException("Viewport entity data is invalid.", nameof(entities));
            }

            foreach (var vertex in vertices)
            {
                if (!float.IsFinite(vertex))
                {
                    throw new ArgumentException("Viewport entity data is invalid.", nameof(entities));
                }
            }

            var owned = vertices.ToArray().AsMemory();
            var ownedEntity = new CadEntityPath(
                entity.EntityId,
                owned,
                entity.PolyWidth,
                entity.IsLine,
                entity.IsMask,
                entity.AnalyticLength);
            copy[index] = ownedEntity;
            if (ownedEntity.AsLine is { } line)
            {
                lines.Add(line);
            }
        }

        for (var index = 0; index < markers.Length; index++)
        {
            var marker = markers[index];
            if (marker.EntityId == 0 ||
                !float.IsFinite(marker.X) ||
                !float.IsFinite(marker.Y) ||
                !ids.Add(marker.EntityId))
            {
                throw new ArgumentException("Viewport marker data is invalid.", nameof(markers));
            }

            markerCopy[index] = marker;
        }

        _entities = copy;
        _markers = markerCopy;
        _lines = lines.ToArray();
        if (_selectedEntityId is { } id && !ids.Contains(id))
        {
            _selectedEntityId = null;
        }

        InvalidateVisual();
    }

    public void SetPreviewVertices(ReadOnlySpan<float> vertices)
    {
        _previewVertices = CopyLineVertices(vertices, nameof(vertices));
        InvalidateVisual();
    }

    public void SetSelectedEntity(ulong? entityId)
    {
        if (entityId == 0 ||
            entityId is { } id &&
            !_entities.Any(entity => entity.EntityId == id) &&
            !_markers.Any(marker => marker.EntityId == id))
        {
            throw new ArgumentOutOfRangeException(nameof(entityId));
        }

        _selectedEntityId = entityId;
        InvalidateVisual();
    }

    public void SetCursor(float x, float y, bool snapped)
    {
        if (!float.IsFinite(x) || !float.IsFinite(y))
        {
            throw new ArgumentOutOfRangeException(nameof(x), "Cursor coordinates must be finite.");
        }

        _cursorX = x;
        _cursorY = y;
        _hasCursor = true;
        _cursorSnapped = snapped;
        InvalidateVisual();
    }

    public void ClearCursor()
    {
        _hasCursor = false;
        _cursorSnapped = false;
        InvalidateVisual();
    }

    public override void Render(DrawingContext context)
    {
        base.Render(context);
        var bounds = new Rect(Bounds.Size);
        context.FillRectangle(Background ?? CanvasBrush, bounds);

        var drawOverlays =
            double.IsFinite(bounds.Width) && bounds.Width > 0 &&
            double.IsFinite(bounds.Height) && bounds.Height > 0;
        if (drawOverlays && ShowGrid)
        {
            DrawGrid(context, bounds, WorldToViewport(0, 0), 17.25 * _zoom);
        }

        var linePen = UseHeavyLineweight ? HeavyLinePen : LinePen;
        foreach (var entity in _entities)
        {
            var entityPen = entity.PolyWidth > 0
                ? new Pen(
                    linePen.Brush,
                    Math.Max(linePen.Thickness, entity.PolyWidth * _zoom),
                    lineCap: PenLineCap.Round,
                    lineJoin: PenLineJoin.Round)
                : linePen;
            if (entity.IsMask)
            {
                DrawMask(context, entityPen, entity);
            }
            else
            {
                DrawEntity(context, entityPen, entity);
            }
            if (ShowDimensions && entity.AsLine is { } line)
            {
                DrawDimension(context, line);
            }
        }

        foreach (var marker in _markers)
        {
            DrawMarker(context, LinePen, marker);
        }

        var selected = _selectedEntityId is { } selectedId
            ? _entities.FirstOrDefault(entity => entity.EntityId == selectedId)
            : default;
        var selectedMarker = _selectedEntityId is { } markerId
            ? _markers.FirstOrDefault(marker => marker.EntityId == markerId)
            : default;
        if (selected.EntityId != 0)
        {
            DrawEntity(context, SelectedPen, selected);
        }
        else if (selectedMarker.EntityId != 0)
        {
            DrawMarker(context, SelectedPen, selectedMarker);
        }

        for (var index = 0; index < _previewVertices.Length; index += 4)
        {
            context.DrawLine(
                PreviewPen,
                WorldToViewport(_previewVertices[index], _previewVertices[index + 1]),
                WorldToViewport(_previewVertices[index + 2], _previewVertices[index + 3]));
        }

        if (selected.EntityId != 0)
        {
            var vertices = selected.Vertices.Span;
            DrawGrip(context, WorldToViewport(vertices[0], vertices[1]));
            if (vertices.Length > 4)
            {
                var middle = (vertices.Length / 4) * 2;
                DrawGrip(context, WorldToViewport(vertices[middle], vertices[middle + 1]));
            }

            if (vertices[^2] != vertices[0] || vertices[^1] != vertices[1])
            {
                DrawGrip(context, WorldToViewport(vertices[^2], vertices[^1]));
            }
        }
        else if (selectedMarker.EntityId != 0)
        {
            DrawGrip(context, WorldToViewport(selectedMarker.X, selectedMarker.Y));
        }

        if (drawOverlays)
        {
            var cursor = _hasCursor
                ? WorldToViewport(_cursorX, _cursorY)
                : _hasPointer
                    ? _pointer
                    : new Point(bounds.Width * 0.61, bounds.Height * 0.402);
            DrawCrosshair(context, cursor);
            if (_hasCursor && _cursorSnapped)
            {
                context.DrawRectangle(null, PreviewPen, new Rect(cursor.X - 5, cursor.Y - 5, 10, 10));
            }
        }

        if (drawOverlays && ShowUcs && bounds.Width >= 64 && bounds.Height >= 64)
        {
            DrawUcs(context, bounds.Height);
        }

        if (BorderBrush is { } border)
        {
            var thickness = BorderThickness;
            context.FillRectangle(border, new Rect(0, 0, thickness.Left, bounds.Height));
            context.FillRectangle(border, new Rect(bounds.Width - thickness.Right, 0, thickness.Right, bounds.Height));
            context.FillRectangle(border, new Rect(0, 0, bounds.Width, thickness.Top));
            context.FillRectangle(border, new Rect(0, bounds.Height - thickness.Bottom, bounds.Width, thickness.Bottom));
        }
    }

    private static void DrawGrip(DrawingContext context, Point point) =>
        context.FillRectangle(GripBrush, new Rect(point.X - 4, point.Y - 4, 8, 8));

    private void DrawEntity(DrawingContext context, Pen pen, CadEntityPath entity)
    {
        var vertices = entity.Vertices.Span;
        for (var index = 2; index < vertices.Length; index += 2)
        {
            context.DrawLine(
                pen,
                WorldToViewport(vertices[index - 2], vertices[index - 1]),
                WorldToViewport(vertices[index], vertices[index + 1]));
        }
    }

    private void DrawMask(DrawingContext context, Pen pen, CadEntityPath entity)
    {
        var vertices = entity.Vertices.Span;
        var geometry = new StreamGeometry();
        using (var geometryContext = geometry.Open())
        {
            geometryContext.BeginFigure(WorldToViewport(vertices[0], vertices[1]), isFilled: true);
            for (var index = 2; index < vertices.Length - 2; index += 2)
            {
                geometryContext.LineTo(WorldToViewport(vertices[index], vertices[index + 1]));
            }

            geometryContext.EndFigure(isClosed: true);
        }

        context.DrawGeometry(Background ?? CanvasBrush, pen, geometry);
    }

    private void DrawMarker(DrawingContext context, Pen pen, CadMarker marker)
    {
        const double radius = 4;
        var point = WorldToViewport(marker.X, marker.Y);
        context.DrawEllipse(null, pen, point, radius, radius);
        context.DrawLine(pen, new Point(point.X - radius - 3, point.Y), new Point(point.X + radius + 3, point.Y));
        context.DrawLine(pen, new Point(point.X, point.Y - radius - 3), new Point(point.X, point.Y + radius + 3));
    }

    private static void DrawGrid(DrawingContext context, Rect bounds, Point origin, double minor)
    {
        while (minor < 7)
        {
            minor *= 4;
        }

        var major = minor * 4;
        var startX = ((origin.X % minor) + minor) % minor;
        var startY = ((origin.Y % minor) + minor) % minor;
        for (var x = startX; x < bounds.Width; x += minor)
        {
            var isMajor = Math.Abs((x - origin.X) % major) < 0.5;
            context.DrawLine(isMajor ? MajorGridPen : MinorGridPen, new Point(x, 0), new Point(x, bounds.Height));
        }

        for (var y = startY; y < bounds.Height; y += minor)
        {
            var isMajor = Math.Abs((y - origin.Y) % major) < 0.5;
            context.DrawLine(isMajor ? MajorGridPen : MinorGridPen, new Point(0, y), new Point(bounds.Width, y));
        }
    }

    private void DrawDimension(DrawingContext context, CadLine line)
    {
        var start = WorldToViewport(line.X1, line.Y1);
        var end = WorldToViewport(line.X2, line.Y2);
        var dx = end.X - start.X;
        var dy = end.Y - start.Y;
        var screenLength = Math.Sqrt(dx * dx + dy * dy);
        if (screenLength < 54)
        {
            return;
        }

        var text = new FormattedText(
            line.Length.ToString("0.##", CultureInfo.InvariantCulture),
            CultureInfo.InvariantCulture,
            FlowDirection.LeftToRight,
            CadTypeface,
            10,
            DimensionBrush);
        var normalX = -dy / screenLength * 9;
        var normalY = dx / screenLength * 9;
        context.DrawText(
            text,
            new Point(
                (start.X + end.X - text.Width) / 2 + normalX,
                (start.Y + end.Y - text.Height) / 2 + normalY));
    }

    private static void DrawUcs(DrawingContext context, double height)
    {
        var origin = new Point(32, height - 73);
        var xEnd = new Point(origin.X + 49, origin.Y);
        var yEnd = new Point(origin.X, origin.Y - 49);

        context.DrawLine(UcsXPen, origin, xEnd);
        context.DrawLine(UcsXPen, xEnd, new Point(xEnd.X - 7, xEnd.Y - 3));
        context.DrawLine(UcsXPen, xEnd, new Point(xEnd.X - 7, xEnd.Y + 3));
        context.DrawLine(UcsYPen, origin, yEnd);
        context.DrawLine(UcsYPen, yEnd, new Point(yEnd.X - 3, yEnd.Y + 7));
        context.DrawLine(UcsYPen, yEnd, new Point(yEnd.X + 3, yEnd.Y + 7));
        context.DrawRectangle(CanvasBrush, UcsOriginPen, new Rect(origin.X - 4, origin.Y - 4, 8, 8));

        context.DrawText(
            new FormattedText("X", CultureInfo.InvariantCulture, FlowDirection.LeftToRight, CadTypeface, 10, UcsXBrush),
            new Point(xEnd.X + 7, xEnd.Y - 7));
        context.DrawText(
            new FormattedText("Y", CultureInfo.InvariantCulture, FlowDirection.LeftToRight, CadTypeface, 10, UcsYBrush),
            new Point(yEnd.X - 4, yEnd.Y - 17));
    }

    private static void DrawCrosshair(DrawingContext context, Point center)
    {
        const double radius = 48;
        var horizontalStart = new Point(center.X - radius, center.Y);
        var horizontalEnd = new Point(center.X + radius, center.Y);
        var verticalStart = new Point(center.X, center.Y - radius);
        var verticalEnd = new Point(center.X, center.Y + radius);

        context.DrawLine(CrosshairPen, horizontalStart, horizontalEnd);
        context.DrawLine(CrosshairPen, verticalStart, verticalEnd);
        context.DrawRectangle(CanvasBrush, CrosshairPen, new Rect(center.X - 4, center.Y - 4, 8, 8));
    }

    private static float[] CopyLineVertices(ReadOnlySpan<float> vertices, string parameterName)
    {
        if (vertices.Length % 4 != 0)
        {
            throw new ArgumentException("Viewport LINE previews require groups of four floats.", parameterName);
        }

        foreach (var vertex in vertices)
        {
            if (!float.IsFinite(vertex))
            {
                throw new ArgumentOutOfRangeException(parameterName, "Viewport coordinates must be finite.");
            }
        }

        return vertices.ToArray();
    }
}
