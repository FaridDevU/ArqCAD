using System.Globalization;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using Avalonia.VisualTree;
using ArcForge.Desktop.Frontend;
using ArcForge.Native;

namespace ArcForge.Desktop;

public sealed partial class MainWindow : Window
{
    private const double HitTolerance = 8.0;
    private const double SnapRadius = 8.0;
    private const double WallHalfWidth = 6.0;
    private const int CircleSegmentCount = 24;
    private const string SaveFirstStatus = "Cambios sin guardar - guarde antes de continuar";
    private const string SaveBeforeCloseStatus = "Cambios sin guardar - guarde antes de cerrar";
    private const string ConnectedStatus = "Motor conectado · LINE listo";
    private const string FirstPointStatus = "LINE · Seleccione el primer punto";
    private const string NextPointStatus = "LINE · Seleccione el siguiente punto";

    private static readonly FilePickerFileType ArcfFileType = new("Documento ArcCAD")
    {
        Patterns = ["*.arcf"],
    };

    private readonly WorkspaceSession _session;
    private readonly CommandSession _commandSession = new();
    private readonly Stack<int> _undoGroups = [];
    private readonly Stack<int> _redoGroups = [];
    private readonly List<ArcCadPoint> _pathPoints = [];
    private LineState _lineState;
    private DrawingTool _drawingTool;
    private ArcCadPoint? _firstPoint;
    private ArcCadPoint? _secondPoint;
    private ArcCadPoint? _thirdPoint;
    private CadLine? _sourceLine;
    private double _toolParameter;
    private double _secondaryToolParameter;
    private double _tertiaryToolParameter;
    private double? _rectangleChamfer1;
    private double? _rectangleChamfer2;
    private double? _rectangleFillet;
    private double? _rectangleWidth;
    private string _revisionCloudStyle = "NORMAL";
    private bool _polygonCircumscribed;
    private ArcCadSnap? _lastSnap;
    private bool _objectSnapEnabled = true;
    private bool _orthoEnabled;
    private bool _panMode;
    private bool _isPanning;
    private Point _panAnchor;
    private ulong? _selectedLayerId;
    private bool _refreshingLayerUi;

    public MainWindow()
        : this(null)
    {
    }

    public MainWindow(string? aliasFilePath)
    {
        InitializeComponent();

        var session = new WorkspaceSession(aliasFilePath);
        try
        {
            _session = session;
            KeyDown += OnWindowKeyDown;
            TitleBar.PointerPressed += OnTitleBarPointerPressed;
            MinimizeButton.Click += (_, _) => WindowState = WindowState.Minimized;
            MaximizeButton.Click += (_, _) => ToggleWindowState();
            CloseButton.Click += (_, _) => Close();
            WorkspaceViewport.PointerPressed += OnViewportPointerPressed;
            WorkspaceViewport.PointerMoved += OnViewportPointerMoved;
            WorkspaceViewport.PointerReleased += OnViewportPointerReleased;
            WorkspaceViewport.PointerWheelChanged += OnViewportPointerWheelChanged;
            CommandInput.KeyDown += OnCommandInputKeyDown;
            Closing += OnClosing;
            Closed += OnClosed;

            DisableUnsupportedEditors();
            SyncScene();
            UpdateWindowChrome();
            UpdateStatus(session.LastAliasError is not null
                ? $"Motor conectado - PGP no cargado: {session.LastAliasError}"
                : session.LastAliasMessage is { } aliasMessage
                    ? $"Motor conectado - {CompactQueryResult(aliasMessage)}"
                    : ConnectedStatus);
        }
        catch
        {
            session.Dispose();
            throw;
        }
    }

    public bool IsBackendConnected => _session.IsBackendConnected;

    public bool IsLineActive => _lineState != LineState.Idle;

    public bool IsAwaitingFirstPoint => _lineState == LineState.AwaitingFirst;

    public ArcCadPoint? PendingFirstPoint => _firstPoint;

    public ArcCadSnap? LastSnap => _lastSnap;

    public ulong LineEntityId => _session.EntityId;

    public ulong LastCreatedEntityId => _session.EntityId;

    public ulong? LastTransactionSequence => _session.LastTransactionSequence;

    public ReadOnlyMemory<CadLine> Lines => _session.Lines;

    public ReadOnlyMemory<CadEntityPath> Entities => _session.Entities;

    public ReadOnlyMemory<CadMarker> Markers => _session.Markers;

    public IReadOnlyList<ArcCadLayerInfo> Layers => _session.Layers;

    public ulong? SelectedLayerId => _selectedLayerId;

    public int EntityCount => _session.EntityCount;

    public ulong? SelectedEntityId => _session.SelectedEntityId;

    public CadLine? SelectedLine => _session.SelectedLine;

    public bool CanUndo => _undoGroups.Count > 0;

    public bool CanRedo => _redoGroups.Count > 0;

    public string? CurrentPath => _session.CurrentPath;

    public bool IsDirty => _session.IsDirty;

    public string AliasFilePath => _session.AliasFilePath;

    public string AliasContent => _session.AliasContent;

    public bool IsObjectSnapEnabled => _objectSnapEnabled;

    public bool IsOrthoEnabled => _orthoEnabled;

    public string? ActiveCommand => _commandSession.ActiveCommand;

    public string CommandPrompt => _commandSession.Prompt;

    public IReadOnlyList<string> CommandOptions => _commandSession.Options;

    public string? CommandDefault => _commandSession.DefaultOption;

    public string? CommandPreview => _commandSession.Preview;

    public string? LastCompletedCommand => _commandSession.LastCompletedCommand;

    public string? LastCommandError => _commandSession.LastError?.Message;

    public string? LastCommandErrorCode => _commandSession.LastError?.Code;

    public IReadOnlyList<CommandHistoryEntry> CommandHistory => _commandSession.History;

    public string ActiveDrawingTool => _drawingTool.ToString();

    public string ActiveRevisionCloudStyle => _revisionCloudStyle;

    public string ActiveRectangleMode => _rectangleFillet.HasValue
        ? "FILLET"
        : _rectangleChamfer1.HasValue
            ? "CHAMFER"
            : "BASIC";

    public double ActiveRectangleWidth => _rectangleWidth ?? 0.0;

    public bool IsPanMode => _panMode;

    public bool AreViewportControlsVisible =>
        ViewOrientationButton.IsVisible && VisualStyleButton.IsVisible && UcsButton.IsVisible;

    public void StartLine() => StartTool(DrawingTool.Line);

    public void StartPolyline() => StartTool(DrawingTool.Polyline);

    public void StartRectangle(
        double? chamfer1 = null,
        double? chamfer2 = null,
        double? fillet = null,
        double? width = null)
    {
        if (_lineState != LineState.Idle || _commandSession.IsActive)
        {
            throw new InvalidOperationException("Ya hay una herramienta de dibujo activa.");
        }

        ValidateOptionalRectangleDistance(chamfer1, nameof(chamfer1));
        ValidateOptionalRectangleDistance(chamfer2, nameof(chamfer2));
        ValidateOptionalRectangleDistance(fillet, nameof(fillet));
        ValidateOptionalRectangleDistance(width, nameof(width));
        if (chamfer1.HasValue != chamfer2.HasValue)
        {
            throw new ArgumentException("RECTANG CHAMFER requiere dos distancias.");
        }

        if (chamfer1.HasValue && fillet.HasValue)
        {
            throw new ArgumentException("RECTANG CHAMFER y FILLET son incompatibles.");
        }

        _rectangleChamfer1 = chamfer1;
        _rectangleChamfer2 = chamfer2;
        _rectangleFillet = fillet;
        _rectangleWidth = width;
        StartTool(DrawingTool.Rectangle);
    }

    public void StartPolygon(int sides, bool circumscribed = false)
    {
        if (sides is < 3 or > 1024)
        {
            throw new ArgumentOutOfRangeException(nameof(sides), "El polígono requiere entre 3 y 1024 lados.");
        }

        _polygonCircumscribed = circumscribed;
        StartTool(DrawingTool.Polygon, parameter: sides);
    }

    public void StartCircle() => StartTool(DrawingTool.Circle);

    public void StartCircleTwoPoint() => StartTool(DrawingTool.CircleTwoPoint);

    public void StartCircleThreePoint() => StartTool(DrawingTool.CircleThreePoint);

    public void StartCircleTtr(double radius)
    {
        if (!double.IsFinite(radius) || radius <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(radius), "CIRCLE TTR requiere un radio positivo.");
        }

        StartTool(DrawingTool.CircleTtr, parameter: radius);
    }

    public void StartArc() => StartTool(DrawingTool.Arc);

    public void StartArcCenterStartEnd() => StartTool(DrawingTool.ArcCenterStartEnd);

    public void StartEllipse() => StartTool(DrawingTool.Ellipse);

    public void StartEllipseCenter(double ratio)
    {
        if (!double.IsFinite(ratio) || ratio <= 0 || ratio > 1)
        {
            throw new ArgumentOutOfRangeException(nameof(ratio), "ELLIPSE C requiere un ratio en (0, 1].");
        }

        StartTool(DrawingTool.EllipseCenter, parameter: ratio);
    }

    public void StartEllipticalArc(double ratio, double startParameterRadians, double endParameterRadians)
    {
        if (!double.IsFinite(ratio) || ratio <= 0 || ratio > 1 ||
            !double.IsFinite(startParameterRadians) || !double.IsFinite(endParameterRadians))
        {
            throw new ArgumentOutOfRangeException(nameof(ratio), "ELLIPSE ARC requiere ratio y parámetros válidos.");
        }

        StartTool(
            DrawingTool.EllipseArc,
            parameter: ratio,
            secondaryParameter: startParameterRadians,
            tertiaryParameter: endParameterRadians);
    }

    public void StartSpline() => StartTool(DrawingTool.Spline);

    public void StartRevisionCloud(double arcLength, string style = "NORMAL")
    {
        if (!double.IsFinite(arcLength) || arcLength <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(arcLength));
        }

        var normalizedStyle = style.Trim().ToUpperInvariant();
        if (normalizedStyle is not ("NORMAL" or "CALLIGRAPHY"))
        {
            throw new ArgumentException(
                "REVCLOUD requiere estilo NORMAL o CALLIGRAPHY.",
                nameof(style));
        }

        _revisionCloudStyle = normalizedStyle;
        StartTool(DrawingTool.RevisionCloud, parameter: arcLength);
    }

    public void StartWipeout() => StartTool(DrawingTool.Wipeout);

    public void StartPoint() => StartTool(DrawingTool.Point);

    public void StartMultiline() => StartTool(DrawingTool.Multiline);

    public void StartMove() => StartTool(DrawingTool.Move, RequireSelectedLine("desplazarla"));

    public void StartRotate() => StartTool(DrawingTool.Rotate, RequireSelectedLine("girarla"));

    public void StartScale(double factor) =>
        StartTool(DrawingTool.Scale, RequireSelectedLine("escalarla"), factor);

    public void StartOffset(double distance) =>
        StartTool(DrawingTool.Offset, RequireSelectedLine("crear su paralela"), distance);

    public void StartTrim() => StartTool(DrawingTool.Trim, RequireSelectedLine("recortarla"));

    public void StartExtend() => StartTool(DrawingTool.Extend, RequireSelectedLine("extenderla"));

    public void StartChamfer(double distance) =>
        StartTool(DrawingTool.Chamfer, RequireSelectedLine("crear el chaflán"), distance);

    public void StartFillet(double radius) =>
        StartTool(DrawingTool.Fillet, RequireSelectedLine("crear el empalme"), radius);

    public void StartBreak() => StartTool(DrawingTool.Break, RequireSelectedLine("partirla"));

    public void StartBreakAtPoint() =>
        StartTool(DrawingTool.BreakAtPoint, RequireSelectedLine("dividirla"));

    public void StartLengthen(double total) =>
        StartTool(DrawingTool.Lengthen, RequireSelectedLine("cambiar su longitud"), total);

    public void StartStretch() => StartTool(DrawingTool.Stretch, RequireSelectedLine("estirarla"));

    public void StartJoin() => StartTool(DrawingTool.Join, RequireSelectedLine("unirla"));

    public void StartAlign() => StartTool(DrawingTool.Align, RequireSelectedLine("alinearla"));

    public void StartCopy() => StartTool(DrawingTool.Copy, RequireSelectedLine("copiar"));

    public void StartMirror() => StartTool(DrawingTool.Mirror, RequireSelectedLine("crear la simetría"));

    public void StartDistance() => StartTool(DrawingTool.Distance);

    public void StartPointId() => StartTool(DrawingTool.PointId);

    public void StartAngle() => StartTool(DrawingTool.Angle);

    public void ListSelectedEntity()
    {
        if (IsLineActive)
        {
            throw new InvalidOperationException("Finalice la herramienta activa antes de usar LIST.");
        }

        UpdateStatus(CompactQueryResult(_session.ListSelectedEntity()));
    }

    public string MeasureSelectedArea()
    {
        if (IsLineActive)
        {
            throw new InvalidOperationException("Finalice la herramienta activa antes de usar AREA.");
        }

        var result = CompactQueryResult(_session.MeasureSelectedArea());
        UpdateStatus(result);
        return result;
    }

    public string MeasureSelectedRadius()
    {
        if (IsLineActive)
        {
            throw new InvalidOperationException("Finalice la herramienta activa antes de medir el radio.");
        }

        var result = CompactQueryResult(_session.MeasureSelectedRadius());
        UpdateStatus(result);
        return result;
    }

    public string MeasureSelectedLength()
    {
        if (IsLineActive)
        {
            throw new InvalidOperationException("Finalice la herramienta activa antes de medir la longitud.");
        }

        var result = CompactQueryResult(_session.MeasureSelectedLength());
        UpdateStatus(result);
        return result;
    }

    public string MeasureSelectedBounds()
    {
        if (IsLineActive)
        {
            throw new InvalidOperationException("Finalice la herramienta activa antes de medir los limites.");
        }

        var result = CompactQueryResult(_session.MeasureSelectedBounds());
        UpdateStatus(result);
        return result;
    }

    public void EraseSelectedEntity()
    {
        if (IsLineActive)
        {
            throw new InvalidOperationException("Finalice la herramienta activa antes de borrar.");
        }

        if (_session.SelectedEntity is null)
        {
            throw new InvalidOperationException("Seleccione una entidad antes de borrarla.");
        }

        CompleteNativeMutation(_session.EraseSelected, "ERASE nativo · entidad eliminada");
    }

    public void RestoreLastErase()
    {
        if (IsLineActive)
        {
            throw new InvalidOperationException("Finalice la herramienta activa antes de usar OOPS.");
        }

        var restored = 0;
        CompleteNativeMutation(
            () => restored = _session.Oops(),
            string.Empty);
        UpdateStatus($"OOPS nativo · {restored} entidad(es) restaurada(s) con ID nuevo");
    }

    public void ExplodeSelectedEntity()
    {
        if (IsLineActive)
        {
            throw new InvalidOperationException("Finalice la herramienta activa antes de usar EXPLODE.");
        }

        CompleteNativeMutation(
            _session.ExplodeSelected,
            "EXPLODE nativo · polilínea descompuesta en tramos independientes");
    }

    public void CreateArray()
    {
        _ = RequireSelectedLine("crear la matriz");
        CompleteNativeMutation(
            () => _session.CreateRectangularArraySelected(3, 2, new ArcCadPoint(36, 36)),
            "Matriz rectangular nativa · 5 copias LINE creadas");
    }

    public void ToggleDimensions()
    {
        WorkspaceViewport.ShowDimensions = !WorkspaceViewport.ShowDimensions;
        UpdateControls();
        UpdateStatus(WorkspaceViewport.ShowDimensions ? "Cotas visibles" : "Cotas ocultas");
    }

    public void ToggleLineweight()
    {
        WorkspaceViewport.UseHeavyLineweight = !WorkspaceViewport.UseHeavyLineweight;
        UpdateControls();
        UpdateStatus(WorkspaceViewport.UseHeavyLineweight ? "Grosor visible" : "Grosor normal");
    }

    public void ZoomIn()
    {
        WorkspaceViewport.ZoomAt(1.25, WorkspaceViewport.Bounds.Center);
        UpdateStatus($"Zoom · {WorkspaceViewport.Zoom:P0}");
    }

    public void ZoomOut()
    {
        WorkspaceViewport.ZoomAt(0.8, WorkspaceViewport.Bounds.Center);
        UpdateStatus($"Zoom · {WorkspaceViewport.Zoom:P0}");
    }

    public void FitView()
    {
        WorkspaceViewport.FitToEntities();
        UpdateStatus($"Dibujo ajustado · {WorkspaceViewport.Zoom:P0}");
    }

    public void ResetView()
    {
        WorkspaceViewport.ResetView();
        UpdateStatus("Vista restablecida · 1:1");
    }

    private void StartTool(
        DrawingTool tool,
        CadLine? source = null,
        double parameter = 0,
        double secondaryParameter = 0,
        double tertiaryParameter = 0)
    {
        if (_lineState != LineState.Idle || _commandSession.IsActive)
        {
            throw new InvalidOperationException("Ya hay una herramienta de dibujo activa.");
        }

        _drawingTool = tool;
        var options = tool switch
        {
            DrawingTool.Polyline => new[] { "CLOSE", "FINISH" },
            DrawingTool.Spline or DrawingTool.Wipeout => new[] { "FINISH" },
            _ => Array.Empty<string>(),
        };
        var defaultOption = tool is DrawingTool.Polyline or DrawingTool.Spline or DrawingTool.Wipeout
            ? "FINISH"
            : null;
        _commandSession.Begin(
            ToolCommandName(tool),
            $"{ToolLabel(tool)} listo",
            options,
            defaultOption,
            $"Vista previa {ToolLabel(tool)}");
        if (tool != DrawingTool.Polygon)
        {
            _polygonCircumscribed = false;
        }
        _sourceLine = source;
        _toolParameter = parameter;
        _secondaryToolParameter = secondaryParameter;
        _tertiaryToolParameter = tertiaryParameter;
        _lineState = LineState.AwaitingFirst;
        _firstPoint = null;
        _secondPoint = null;
        _thirdPoint = null;
        _pathPoints.Clear();
        _lastSnap = null;
        WorkspaceViewport.SetPreviewVertices([]);
        WorkspaceViewport.ClearCursor();
        SetPanMode(false);
        UpdateControls();
        UpdateStatus(tool switch
        {
            DrawingTool.Line => FirstPointStatus,
            DrawingTool.Angle => "Ángulo · Seleccione el vértice",
            DrawingTool.Rotate => "Girar · Seleccione el punto base",
            DrawingTool.Scale => $"SCALE ×{parameter.ToString("G", CultureInfo.InvariantCulture)} · Seleccione el punto base",
            DrawingTool.Offset => $"OFFSET {parameter.ToString("G", CultureInfo.InvariantCulture)} · Seleccione el lado",
            DrawingTool.Rectangle => $"RECTANG {RectangleModifierLabel()} · Seleccione la primera esquina",
            DrawingTool.Polygon =>
                $"POLYGON {(int)parameter} · Seleccione el centro ({(_polygonCircumscribed ? "circunscrito" : "inscrito")})",
            DrawingTool.Donut =>
                $"DONUT · interior {secondaryParameter:G} · exterior {parameter:G} · Seleccione el centro",
            DrawingTool.CircleTtr =>
                $"CIRCLE TTR R{parameter.ToString("G", CultureInfo.InvariantCulture)} · Seleccione la primera LINE cerca de la tangencia",
            DrawingTool.CircleTwoPoint => "CIRCLE 2P · Seleccione el primer extremo del diámetro",
            DrawingTool.CircleThreePoint => "CIRCLE 3P · Seleccione el primer punto",
            DrawingTool.ArcCenterStartEnd => "ARC CSE · Seleccione el centro",
            DrawingTool.EllipseCenter =>
                $"ELLIPSE C · ratio {parameter.ToString("G", CultureInfo.InvariantCulture)} · Seleccione el centro",
            DrawingTool.EllipseArc =>
                $"ELLIPSE ARC · ratio {parameter.ToString("G", CultureInfo.InvariantCulture)} · Seleccione el centro",
            DrawingTool.Point => "POINT · Seleccione la posición",
            DrawingTool.RevisionCloud =>
                $"REVCLOUD · arco {parameter.ToString("G", CultureInfo.InvariantCulture)} · Seleccione la primera esquina",
            DrawingTool.Wipeout => "WIPEOUT · Seleccione el primer vértice",
            DrawingTool.Trim => "TRIM · Señale la parte que desea recortar",
            DrawingTool.Extend => "EXTEND · Señale el extremo que desea alargar",
            DrawingTool.Chamfer => $"CHAMFER {parameter.ToString("G", CultureInfo.InvariantCulture)} · Seleccione la segunda LINE",
            DrawingTool.Fillet => $"FILLET R{parameter.ToString("G", CultureInfo.InvariantCulture)} · Seleccione la segunda LINE",
            DrawingTool.Break => "BREAK · Seleccione el primer punto del hueco",
            DrawingTool.BreakAtPoint => "BREAKATPOINT · Seleccione el punto de división",
            DrawingTool.Lengthen => $"LENGTHEN {parameter.ToString("G", CultureInfo.InvariantCulture)} · Seleccione el extremo",
            DrawingTool.Stretch => "STRETCH · Seleccione la primera esquina de captura",
            DrawingTool.Join => "JOIN · Seleccione la segunda LINE colineal",
            DrawingTool.Align => "ALIGN · Seleccione el primer punto de origen",
            _ => $"{ToolLabel(tool)} · Seleccione el primer punto",
        });
    }

    public void AcceptPoint(ArcCadPoint raw)
    {
        if (_lineState == LineState.AwaitingFirst)
        {
            if (_drawingTool == DrawingTool.CircleTtr)
            {
                _session.SelectAt(raw, HitTolerance / WorkspaceViewport.Zoom);
                SyncSelection();
                UpdateControls();
                _sourceLine = _session.SelectedLine ??
                    throw new InvalidOperationException("Seleccione una primera LINE.");
                _firstPoint = raw;
                _lineState = LineState.AwaitingNext;
                SetCursor(raw);
                UpdateStatus(
                    $"CIRCLE TTR R{_toolParameter.ToString("G", CultureInfo.InvariantCulture)} · Seleccione la segunda LINE cerca de la tangencia");
                return;
            }

            if (_drawingTool is DrawingTool.Chamfer or DrawingTool.Fillet or DrawingTool.Join)
            {
                var source = _sourceLine ?? throw new InvalidOperationException("No hay LINE de origen.");
                _ = SelectOtherLine(raw);
                if (_drawingTool == DrawingTool.Chamfer)
                {
                    CompleteNativeMutation(
                        () => _session.ChamferSelectedWith(source.EntityId, _toolParameter),
                        $"CHAMFER nativo · distancia {_toolParameter.ToString("G", CultureInfo.InvariantCulture)}");
                }
                else if (_drawingTool == DrawingTool.Fillet)
                {
                    CompleteNativeMutation(
                        () => _session.FilletSelectedWith(source.EntityId, _toolParameter),
                        $"FILLET nativo · radio {_toolParameter.ToString("G", CultureInfo.InvariantCulture)} · ARC tangente creado");
                }
                else
                {
                    CompleteNativeMutation(
                        () => _session.JoinSelectedWith(source.EntityId),
                        "JOIN nativo · LINE colineales unidas");
                }
                return;
            }

            var first = ResolvePoint(raw);
            if (_drawingTool == DrawingTool.PointId)
            {
                CompleteQuery(_session.IdentifyPoint(first));
                return;
            }

            switch (_drawingTool)
            {
                case DrawingTool.Point:
                    CompleteNativeMutation(
                        () => _session.CreatePoint(first),
                        "POINT nativo · 1 entidad puntual");
                    return;
                case DrawingTool.Scale:
                    CompleteNativeMutation(
                        () => _session.ScaleSelected(first, _toolParameter),
                        $"SCALE nativo · factor {_toolParameter.ToString("G", CultureInfo.InvariantCulture)}");
                    return;
                case DrawingTool.Offset:
                    CompleteNativeMutation(
                        () => _session.OffsetSelected(_toolParameter, first),
                        $"OFFSET nativo · distancia {_toolParameter.ToString("G", CultureInfo.InvariantCulture)}");
                    return;
                case DrawingTool.Trim:
                    CompleteNativeMutation(
                        () => _session.TrimSelected(first),
                        "TRIM nativo · LINE recortada");
                    return;
                case DrawingTool.Extend:
                    CompleteNativeMutation(
                        () => _session.ExtendSelected(first),
                        "EXTEND nativo · LINE extendida");
                    return;
                case DrawingTool.BreakAtPoint:
                    CompleteNativeMutation(
                        () => _session.BreakSelectedAtPoint(first),
                        "BREAKATPOINT nativo · LINE dividida");
                    return;
                case DrawingTool.Lengthen:
                    CompleteNativeMutation(
                        () => _session.LengthenSelected(first, _toolParameter),
                        $"LENGTHEN nativo · total {_toolParameter.ToString("G", CultureInfo.InvariantCulture)}");
                    return;
                case DrawingTool.XlineHorizontal:
                    CompleteNativeMutation(
                        () => _session.CreateHorizontalXline(first),
                        "XLINE horizontal nativa · 1 entidad");
                    return;
                case DrawingTool.XlineVertical:
                    CompleteNativeMutation(
                        () => _session.CreateVerticalXline(first),
                        "XLINE vertical nativa · 1 entidad");
                    return;
                case DrawingTool.XlineAngle:
                    CompleteNativeMutation(
                        () => _session.CreateAngledXline(first, _toolParameter),
                        $"XLINE angular nativa · {_toolParameter * 180 / Math.PI:F2}° · 1 entidad");
                    return;
                case DrawingTool.Donut:
                    CompleteNativeMutation(
                        () => _session.CreateDonut(first, _toolParameter, _secondaryToolParameter),
                        $"DONUT nativo · Ø{_secondaryToolParameter:G}/Ø{_toolParameter:G} · 1 entidad");
                    return;
            }

            _firstPoint = first;
            if (_drawingTool is DrawingTool.Polyline or DrawingTool.Spline or DrawingTool.Wipeout)
            {
                _pathPoints.Add(first);
            }
            _lineState = LineState.AwaitingNext;
            SetCursor(first);
            UpdateStatus(_drawingTool switch
            {
                DrawingTool.Line => NextPointStatus,
                DrawingTool.Angle => "Ángulo · Seleccione un punto del primer rayo",
                DrawingTool.Rotate => "Girar · Seleccione el punto de referencia",
                DrawingTool.Break => "BREAK · Seleccione el segundo punto del hueco",
                DrawingTool.Stretch => "STRETCH · Seleccione la esquina opuesta",
                DrawingTool.Align => "ALIGN · Seleccione el primer punto de destino",
                DrawingTool.Polygon => "POLYGON · Seleccione el punto de radio",
                DrawingTool.Polyline => "PLINE · Seleccione el siguiente vértice · Enter/Esc termina · C cierra",
                DrawingTool.Spline => "SPLINE · Seleccione puntos de ajuste · Enter/Esc termina",
                DrawingTool.Wipeout => "WIPEOUT · Seleccione vértices · Enter/Esc confirma desde 3",
                DrawingTool.Ellipse => "ELLIPSE · Seleccione el extremo del semieje mayor",
                DrawingTool.EllipseCenter => "ELLIPSE C · Seleccione el extremo del semieje mayor",
                DrawingTool.EllipseArc => "ELLIPSE ARC · Seleccione el extremo del semieje mayor",
                DrawingTool.ArcCenterStartEnd => "ARC CSE · Seleccione el punto inicial",
                _ => $"{ToolLabel(_drawingTool)} · Seleccione el segundo punto",
            });
            return;
        }

        if (_lineState == LineState.AwaitingFourth &&
            _drawingTool == DrawingTool.Stretch &&
            _firstPoint is { } firstCorner &&
            _secondPoint is { } secondCorner &&
            _thirdPoint is { } basePoint)
        {
            var destination = ResolvePoint(raw);
            ValidateSegment(basePoint, destination);
            CompleteNativeMutation(
                () => _session.StretchSelected(firstCorner, secondCorner, basePoint, destination),
                "STRETCH nativo · vértice de LINE desplazado");
            return;
        }

        if (_lineState == LineState.AwaitingFourth &&
            _drawingTool == DrawingTool.Align &&
            _firstPoint is { } firstSource &&
            _secondPoint is { } firstDestination &&
            _thirdPoint is { } secondSource)
        {
            var secondDestination = ResolvePoint(raw);
            CompleteNativeMutation(
                () => _session.AlignSelected(
                    firstSource,
                    firstDestination,
                    secondSource,
                    secondDestination),
                "ALIGN nativo · LINE alineada por dos pares");
            return;
        }

        if (_lineState == LineState.AwaitingThird &&
            _firstPoint is { } thirdStart &&
            _secondPoint is { } thirdMiddle)
        {
            var thirdEnd = ResolvePoint(raw);
            if (_drawingTool == DrawingTool.Stretch)
            {
                _thirdPoint = thirdEnd;
                _lineState = LineState.AwaitingFourth;
                SetCursor(thirdEnd);
                WorkspaceViewport.SetPreviewVertices(Flatten(RectangleSegments(thirdStart, thirdMiddle)));
                UpdateStatus("STRETCH · Seleccione el punto de destino");
                return;
            }

            if (_drawingTool == DrawingTool.Align)
            {
                _thirdPoint = thirdEnd;
                _lineState = LineState.AwaitingFourth;
                SetCursor(thirdEnd);
                WorkspaceViewport.SetPreviewVertices(LineVertices(thirdStart, thirdMiddle));
                UpdateStatus("ALIGN · Seleccione el segundo punto de destino");
                return;
            }

            if (_drawingTool == DrawingTool.Arc)
            {
                CompleteNativeMutation(
                    () => _session.CreateArc(thirdStart, thirdMiddle, thirdEnd),
                    "ARC nativo · 1 entidad curva");
                return;
            }

            if (_drawingTool == DrawingTool.CircleThreePoint)
            {
                CompleteNativeMutation(
                    () => _session.CreateCircleThreePoints(thirdStart, thirdMiddle, thirdEnd),
                    "CIRCLE 3P nativo · circuncírculo creado");
                return;
            }

            if (_drawingTool == DrawingTool.ArcCenterStartEnd)
            {
                CompleteNativeMutation(
                    () => _session.CreateArcCenterStartEnd(thirdStart, thirdMiddle, thirdEnd),
                    "ARC CSE nativo · arco CCW creado");
                return;
            }

            if (_drawingTool == DrawingTool.Ellipse)
            {
                var ratio = EllipseRatio(thirdStart, thirdMiddle, thirdEnd);
                CompleteNativeMutation(
                    () => _session.CreateEllipse(thirdStart, thirdMiddle, ratio),
                    $"ELLIPSE nativa · ratio {ratio:F4} · 1 entidad");
                return;
            }

            if (_drawingTool == DrawingTool.Angle)
            {
                CompleteQuery(_session.MeasureAngle(thirdStart, thirdMiddle, thirdEnd));
                return;
            }

            if (_drawingTool == DrawingTool.Rotate)
            {
                var angle = RotationAngle(thirdStart, thirdMiddle, thirdEnd);
                CompleteNativeMutation(
                    () => _session.RotateSelected(thirdStart, angle),
                    $"ROTATE nativo · {angle * 180 / Math.PI:F2}°");
                return;
            }
        }

        if (_lineState != LineState.AwaitingNext || _firstPoint is not { } start)
        {
            throw new InvalidOperationException("La herramienta no está esperando ese punto.");
        }

        if (_drawingTool == DrawingTool.CircleTtr)
        {
            var firstLine = _sourceLine ?? throw new InvalidOperationException("No hay primera LINE.");
            var secondLine = SelectOtherLine(raw);
            CompleteNativeMutation(
                () => _session.CreateTangentCircle(
                    firstLine.EntityId,
                    start,
                    secondLine.EntityId,
                    raw,
                    _toolParameter),
                $"CIRCLE TTR nativo · radio {_toolParameter.ToString("G", CultureInfo.InvariantCulture)} · círculo tangente creado");
            return;
        }

        var end = ResolvePoint(raw);
        switch (_drawingTool)
        {
            case DrawingTool.Line:
                CreateContinuous([Segment(start, end)], end, ToolLabel(_drawingTool));
                return;
            case DrawingTool.Polyline:
                ValidateSegment(start, end);
                _pathPoints.Add(end);
                _firstPoint = end;
                WorkspaceViewport.SetPreviewVertices(PathPreview());
                SetCursor(end);
                UpdateStatus($"PLINE · {_pathPoints.Count} vértices · Enter/Esc termina · C cierra");
                return;
            case DrawingTool.Spline:
                ValidateSegment(start, end);
                _pathPoints.Add(end);
                _firstPoint = end;
                WorkspaceViewport.SetPreviewVertices(PathPreview());
                SetCursor(end);
                UpdateStatus($"SPLINE · {_pathPoints.Count} puntos · Enter/Esc termina");
                return;
            case DrawingTool.Wipeout:
                ValidateSegment(start, end);
                _pathPoints.Add(end);
                _firstPoint = end;
                WorkspaceViewport.SetPreviewVertices(WipeoutPreview());
                SetCursor(end);
                UpdateStatus($"WIPEOUT · {_pathPoints.Count} vértices · Enter/Esc confirma desde 3");
                return;
            case DrawingTool.Xline:
                ValidateSegment(start, end);
                CompleteNativeMutation(
                    () => _session.CreateXline(start, end),
                    "XLINE nativa · 1 entidad infinita");
                return;
            case DrawingTool.Ray:
                ValidateSegment(start, end);
                CompleteNativeMutation(
                    () => _session.CreateRay(start, end),
                    "RAY nativa · 1 semirrecta");
                return;
            case DrawingTool.RevisionCloud:
                var contour = RevisionCloudContour(start, end);
                CompleteNativeMutation(
                    () => _session.CreateRevisionCloud(contour, _toolParameter, _revisionCloudStyle),
                    $"REVCLOUD nativa · arco {_toolParameter.ToString("G", CultureInfo.InvariantCulture)} · 1 entidad");
                return;
            case DrawingTool.Rectangle:
                _ = RectanglePreviewSegments(start, end);
                CompleteNativeMutation(
                    () => _session.CreateRectangle(
                        start,
                        end,
                        _rectangleChamfer1,
                        _rectangleChamfer2,
                        _rectangleFillet,
                        _rectangleWidth),
                    $"RECTANG {RectangleModifierLabel()} nativo · 1 polilínea cerrada");
                return;
            case DrawingTool.Polygon:
                CompleteNativeMutation(
                    () => _session.CreatePolygon(
                        (int)_toolParameter,
                        start,
                        end,
                        _polygonCircumscribed),
                    $"POLYGON nativo · {(int)_toolParameter} lados · 1 polilínea cerrada");
                return;
            case DrawingTool.Circle:
                CompleteNativeMutation(
                    () => _session.CreateCircle(start, end),
                    "CIRCLE nativo · 1 entidad curva");
                return;
            case DrawingTool.CircleTwoPoint:
                CompleteNativeMutation(
                    () => _session.CreateCircleTwoPoints(start, end),
                    "CIRCLE 2P nativo · diámetro por dos puntos");
                return;
            case DrawingTool.CircleThreePoint:
                _secondPoint = end;
                _lineState = LineState.AwaitingThird;
                SetCursor(end);
                WorkspaceViewport.SetPreviewVertices(LineVertices(start, end));
                UpdateStatus("CIRCLE 3P · Seleccione el tercer punto");
                return;
            case DrawingTool.Arc:
                ValidateSegment(start, end);
                _secondPoint = end;
                _lineState = LineState.AwaitingThird;
                SetCursor(end);
                WorkspaceViewport.SetPreviewVertices(LineVertices(start, end));
                UpdateStatus("Arco · Seleccione el tercer punto");
                return;
            case DrawingTool.ArcCenterStartEnd:
                _secondPoint = end;
                _lineState = LineState.AwaitingThird;
                SetCursor(end);
                WorkspaceViewport.SetPreviewVertices(LineVertices(start, end));
                UpdateStatus("ARC CSE · Seleccione el punto final");
                return;
            case DrawingTool.EllipseCenter:
                CompleteNativeMutation(
                    () => _session.CreateEllipse(start, end, _toolParameter),
                    $"ELLIPSE C nativa · ratio {_toolParameter.ToString("G", CultureInfo.InvariantCulture)} · 1 entidad");
                return;
            case DrawingTool.EllipseArc:
                CompleteNativeMutation(
                    () => _session.CreateEllipticalArc(
                        start,
                        end,
                        _toolParameter,
                        _secondaryToolParameter,
                        _tertiaryToolParameter),
                    $"ELLIPSE ARC nativa · ratio {_toolParameter.ToString("G", CultureInfo.InvariantCulture)} · 1 entidad");
                return;
            case DrawingTool.Ellipse:
                ValidateSegment(start, end);
                _secondPoint = end;
                _lineState = LineState.AwaitingThird;
                SetCursor(end);
                WorkspaceViewport.SetPreviewVertices(LineVertices(start, end));
                UpdateStatus("ELLIPSE · Indique el semieje menor perpendicular");
                return;
            case DrawingTool.Distance:
                CompleteQuery(_session.MeasureDistance(start, end));
                return;
            case DrawingTool.Angle:
                ValidateSegment(start, end);
                _secondPoint = end;
                _lineState = LineState.AwaitingThird;
                SetCursor(end);
                WorkspaceViewport.SetPreviewVertices(LineVertices(start, end));
                UpdateStatus("Ángulo · Seleccione un punto del segundo rayo");
                return;
            case DrawingTool.Rotate:
                ValidateSegment(start, end);
                _secondPoint = end;
                _lineState = LineState.AwaitingThird;
                SetCursor(end);
                WorkspaceViewport.SetPreviewVertices(LineVertices(start, end));
                UpdateStatus("Girar · Seleccione el nuevo ángulo");
                return;
            case DrawingTool.Multiline:
                CreateContinuous(MultilineSegments(start, end), end, "Muro doble");
                return;
            case DrawingTool.Move:
                ValidateSegment(start, end);
                CompleteNativeMutation(
                    () => _session.MoveSelected(start, end),
                    "MOVE nativo · LINE desplazada");
                return;
            case DrawingTool.Copy:
                ValidateSegment(start, end);
                CompleteNativeMutation(
                    () => _session.CopySelected(start, end),
                    "COPY nativo · LINE creada");
                return;
            case DrawingTool.Mirror:
                ValidateSegment(start, end);
                CompleteNativeMutation(
                    () => _session.MirrorSelected(start, end),
                    "MIRROR nativo · LINE creada");
                return;
            case DrawingTool.Break:
                ValidateSegment(start, end);
                CompleteNativeMutation(
                    () => _session.BreakSelected(start, end),
                    "BREAK nativo · tramo eliminado");
                return;
            case DrawingTool.Stretch:
                ValidateSegment(start, end);
                _secondPoint = end;
                _lineState = LineState.AwaitingThird;
                SetCursor(end);
                WorkspaceViewport.SetPreviewVertices(Flatten(RectangleSegments(start, end)));
                UpdateStatus("STRETCH · Seleccione el punto base");
                return;
            case DrawingTool.Align:
                _secondPoint = end;
                _lineState = LineState.AwaitingThird;
                SetCursor(end);
                WorkspaceViewport.SetPreviewVertices(LineVertices(start, end));
                UpdateStatus("ALIGN · Seleccione el segundo punto de origen");
                return;
            default:
                throw new InvalidOperationException("La herramienta activa no tiene una operación válida.");
        }
    }

    public void MovePointer(ArcCadPoint raw)
    {
        if (!IsLineActive)
        {
            return;
        }

        var point = ResolvePoint(raw);
        SetCursor(point);
        if (_lineState == LineState.AwaitingFirst && _drawingTool == DrawingTool.Donut)
        {
            WorkspaceViewport.SetPreviewVertices(Flatten(CircleSegments(
                point,
                new ArcCadPoint(point.X + _toolParameter / 2, point.Y))));
        }
        else if (_lineState == LineState.AwaitingNext &&
            _drawingTool is DrawingTool.Polyline or DrawingTool.Spline or DrawingTool.Wipeout &&
            _firstPoint is not null)
        {
            WorkspaceViewport.SetPreviewVertices(_drawingTool == DrawingTool.Wipeout
                ? WipeoutPreview(point)
                : PathPreview(point));
        }
        else if (_lineState == LineState.AwaitingNext && _firstPoint is { } start)
        {
            WorkspaceViewport.SetPreviewVertices(PreviewVertices(start, point));
        }
        else if (_lineState == LineState.AwaitingThird &&
            _firstPoint is { } thirdStart &&
            _secondPoint is { } thirdMiddle)
        {
            if (_drawingTool == DrawingTool.Stretch)
            {
                WorkspaceViewport.SetPreviewVertices(Flatten(RectangleSegments(thirdStart, thirdMiddle)));
                return;
            }

            if (_drawingTool == DrawingTool.Align)
            {
                WorkspaceViewport.SetPreviewVertices(Flatten([
                    Segment(thirdStart, thirdMiddle),
                    Segment(thirdStart, point),
                ]));
                return;
            }

            if (_drawingTool is DrawingTool.Angle or DrawingTool.Rotate)
            {
                WorkspaceViewport.SetPreviewVertices(Flatten([
                    Segment(thirdStart, thirdMiddle),
                    Segment(thirdStart, point),
                ]));
                return;
            }

            if (_drawingTool == DrawingTool.Ellipse)
            {
                try
                {
                    WorkspaceViewport.SetPreviewVertices(Flatten(
                        EllipseSegments(thirdStart, thirdMiddle, point)));
                }
                catch (ArgumentException)
                {
                    WorkspaceViewport.SetPreviewVertices(LineVertices(thirdStart, thirdMiddle));
                }
                return;
            }

            try
            {
                WorkspaceViewport.SetPreviewVertices(Flatten(ArcSegments(thirdStart, thirdMiddle, point)));
            }
            catch (ArgumentException)
            {
                WorkspaceViewport.SetPreviewVertices(Flatten([
                    Segment(thirdStart, thirdMiddle),
                    Segment(thirdMiddle, point),
                ]));
            }
        }
        else if (_lineState == LineState.AwaitingFourth &&
            _drawingTool == DrawingTool.Stretch &&
            _firstPoint is { } firstCorner &&
            _secondPoint is { } secondCorner &&
            _thirdPoint is { } basePoint)
        {
            WorkspaceViewport.SetPreviewVertices(Flatten([
                .. RectangleSegments(firstCorner, secondCorner),
                Segment(basePoint, point),
            ]));
        }
        else if (_lineState == LineState.AwaitingFourth &&
            _drawingTool == DrawingTool.Align &&
            _firstPoint is { } firstSource &&
            _secondPoint is { } firstDestination &&
            _thirdPoint is { } secondSource)
        {
            WorkspaceViewport.SetPreviewVertices(Flatten([
                Segment(firstSource, secondSource),
                Segment(firstDestination, point),
            ]));
        }
    }

    public ArcCadPoint ResolvePoint(ArcCadPoint raw)
    {
        var resolved = raw;
        _lastSnap = null;
        if (_objectSnapEnabled)
        {
            resolved = _session.ResolvePoint(raw, SnapRadius / WorkspaceViewport.Zoom, out var snap);
            _lastSnap = snap;
        }

        var orthoOrigin = _drawingTool switch
        {
            DrawingTool.Stretch => _lineState == LineState.AwaitingFourth ? _thirdPoint : null,
            DrawingTool.Align => null,
            _ => _firstPoint,
        };
        if (_orthoEnabled && orthoOrigin is { } origin && _lastSnap is null)
        {
            resolved = Math.Abs(resolved.X - origin.X) >= Math.Abs(resolved.Y - origin.Y)
                ? new ArcCadPoint(resolved.X, origin.Y)
                : new ArcCadPoint(origin.X, resolved.Y);
        }

        return resolved;
    }

    public void SelectAt(ArcCadPoint point)
    {
        if (_lineState != LineState.Idle)
        {
            throw new InvalidOperationException("Finish LINE before selecting.");
        }

        _session.SelectAt(point, HitTolerance / WorkspaceViewport.Zoom);
        SyncSelection();
        UpdateControls();
    }

    public void Undo()
    {
        CancelLine();
        var count = _undoGroups.Pop();
        for (var index = 0; index < count; index++)
        {
            _session.Undo();
        }
        _redoGroups.Push(count);
        SyncScene();
        UpdateStatus($"Motor conectado · {_session.EntityCount} entidades tras deshacer");
    }

    public void Redo()
    {
        CancelLine();
        var count = _redoGroups.Pop();
        for (var index = 0; index < count; index++)
        {
            _session.Redo();
        }
        _undoGroups.Push(count);
        SyncScene();
        UpdateStatus($"Motor conectado · {_session.EntityCount} entidades tras rehacer");
    }

    public void NewDocument()
    {
        _commandSession.Cancel("Comando cancelado por documento nuevo");
        _session.NewDocument();
        ResetHistoryGroups();
        ResetLineState();
        WorkspaceViewport.ResetView();
        SyncScene();
        UpdateStatus("Documento nuevo");
    }

    public void SaveToPath(string path)
    {
        _session.SaveToPath(path);
        UpdateControls();
        UpdateStatus($"Guardado - {Path.GetFileName(_session.CurrentPath)}");
    }

    public IReadOnlyList<string> OpenFromPath(string path)
    {
        _commandSession.Cancel("Comando cancelado por apertura");
        var warnings = _session.OpenFromPath(path);
        ResetHistoryGroups();
        ResetLineState();
        WorkspaceViewport.ResetView();
        SyncScene();
        UpdateStatus(warnings.Count == 0
            ? $"Abierto - {_session.EntityCount} entidades"
            : $"Abierto con {warnings.Count} advertencias");
        return warnings;
    }

    public bool HandleKey(Key key, KeyModifiers modifiers)
    {
        if (modifiers == KeyModifiers.None && key == Key.Enter)
        {
            if (_drawingTool == DrawingTool.Polyline && _pathPoints.Count >= 2)
            {
                FinishPolyline();
                return true;
            }

            if (_drawingTool == DrawingTool.Spline && _pathPoints.Count >= 3)
            {
                FinishSpline();
                return true;
            }

            if (_drawingTool == DrawingTool.Wipeout && _pathPoints.Count >= 3)
            {
                FinishWipeout();
                return true;
            }
        }

        if (key == Key.Escape && modifiers == KeyModifiers.None)
        {
            if (ModalOverlay.IsVisible)
            {
                ModalOverlay.IsVisible = false;
                return true;
            }

            if (IsLineActive)
            {
                CancelLine();
                return true;
            }

            if (_panMode)
            {
                SetPanMode(false);
                UpdateControls();
                UpdateStatus(ConnectedStatus);
                return true;
            }
        }

        if (key == Key.Delete && modifiers == KeyModifiers.None)
        {
            ExecuteErase();
            return true;
        }

        if (modifiers != KeyModifiers.Control)
        {
            return false;
        }

        switch (key)
        {
            case Key.N:
                ExecuteNew();
                return true;
            case Key.O:
                _ = OpenDocumentAsync();
                return true;
            case Key.S:
                _ = SaveDocumentAsync();
                return true;
            case Key.Z when CanUndo:
                Undo();
                return true;
            case Key.Y when CanRedo:
                Redo();
                return true;
            default:
                return false;
        }
    }

    public void CancelLine()
    {
        if (!IsLineActive)
        {
            return;
        }

        _commandSession.Cancel();
        ResetLineState();
        UpdateControls();
        UpdateStatus(ConnectedStatus);
    }

    private void OnRootPointerPressed(object? sender, PointerPressedEventArgs eventArgs)
    {
        if (!ReferenceEquals(eventArgs.Source, RootOverlayGrid))
        {
            return;
        }

        SearchResultsPanel.IsVisible = false;
        TopSurfacePanel.IsVisible = false;
        ToastPanel.IsVisible = false;
    }

    private async void OnUiActionClick(object? sender, RoutedEventArgs eventArgs)
    {
        if (sender is Control control)
        {
            await RouteActionAsync(control);
            eventArgs.Handled = true;
        }
    }

    private void OnRibbonCommandClick(object? sender, RoutedEventArgs eventArgs) =>
        OnUiActionClick(sender, eventArgs);

    private void OnRibbonCommandMenuClick(object? sender, RoutedEventArgs eventArgs) =>
        OnUiActionClick(sender, eventArgs);

    private void OnLayerRowsToggleClick(object? sender, RoutedEventArgs eventArgs)
    {
        LayerRowsPanel.IsVisible = !LayerRowsPanel.IsVisible;
        LayerRowsToggleIcon.Tool = LayerRowsPanel.IsVisible ? "ChevronDown" : "ChevronRight";
        eventArgs.Handled = true;
    }

    private void OnCurrentLayerSelectionChanged(object? sender, SelectionChangedEventArgs eventArgs)
    {
        if (_refreshingLayerUi || CurrentLayerCombo.SelectedIndex < 0)
        {
            return;
        }

        var layers = _session.Layers;
        if (CurrentLayerCombo.SelectedIndex >= layers.Count)
        {
            RefreshLayerPanel();
            return;
        }

        var layer = layers[CurrentLayerCombo.SelectedIndex];
        _selectedLayerId = layer.Id;
        if (layer.Current)
        {
            RefreshLayerPanel();
            return;
        }

        try
        {
            CompleteNativeMutation(
                () => _session.SetCurrentLayer(layer.Id),
                $"LAYER nativo - capa actual: {layer.Name}");
        }
        catch (Exception error)
        {
            RefreshLayerPanel();
            UpdateStatus($"No se pudo cambiar la capa actual - {error.Message}");
        }
    }

    private void OnModalOverlayPointerPressed(object? sender, PointerPressedEventArgs eventArgs)
    {
        if (ReferenceEquals(eventArgs.Source, ModalOverlay))
        {
            ModalOverlay.IsVisible = false;
            eventArgs.Handled = true;
        }
    }

    private void OnModalCancelClick(object? sender, RoutedEventArgs eventArgs)
    {
        ModalOverlay.IsVisible = false;
        eventArgs.Handled = true;
    }

    private void OnModalApplyClick(object? sender, RoutedEventArgs eventArgs)
    {
        ModalOverlay.IsVisible = false;
        UpdateUnavailable("acción modal");
        eventArgs.Handled = true;
    }

    private void OnModalConfirmClick(object? sender, RoutedEventArgs eventArgs)
    {
        ModalOverlay.IsVisible = false;
        UpdateUnavailable("acción modal");
        eventArgs.Handled = true;
    }

    private async Task RouteActionAsync(Control source)
    {
        var action = source.Tag?.ToString();
        if (action?.StartsWith("ribbon.tab.", StringComparison.Ordinal) == true)
        {
            ShowRibbonTab(action);
            return;
        }

        if (action?.StartsWith("layers.", StringComparison.Ordinal) == true)
        {
            ExecuteLayerUiAction(action);
            return;
        }

        switch (action)
        {
            case "qat.new":
            case "shell.new-document":
            case "document.new":
                ExecuteNew();
                return;
            case "qat.open":
                await OpenDocumentAsync();
                return;
            case "qat.save":
                await SaveDocumentAsync();
                return;
            case "qat.undo" when CanUndo:
                Undo();
                return;
            case "qat.redo" when CanRedo:
                Redo();
                return;
            case "command.previous":
                CommandInput.Text = _commandSession.PreviousHistoryCommand();
                CommandInput.Focus();
                return;
            case "command.next":
                CommandInput.Text = _commandSession.NextHistoryCommand();
                CommandInput.Focus();
                return;
            case "command.history":
                UpdateStatus(_commandSession.History.Count == 0
                    ? "Historial de comandos vacio"
                    : $"Historial · {string.Join(" · ", _commandSession.History.TakeLast(5).Select(entry => $"{entry.Command}:{entry.Outcome}"))}");
                return;
            case "command.options":
                ExecuteFirstCommandOption();
                return;
            case "home.draw.line":
            case "workspace.tool.line":
                ExecuteDrawingTool(DrawingTool.Line);
                return;
            case "home.draw.polyline":
            case "workspace.tool.polyline":
                ExecuteDrawingTool(DrawingTool.Polyline);
                return;
            case "home.draw.rectangle":
                ExecuteDrawingTool(DrawingTool.Rectangle);
                return;
            case "home.draw.circle":
            case "workspace.tool.circle":
                ExecuteDrawingTool(DrawingTool.Circle);
                return;
            case "home.draw.arc":
            case "workspace.tool.arc":
                ExecuteDrawingTool(DrawingTool.Arc);
                return;
            case "home.annotate.multiline":
                ExecuteDrawingTool(DrawingTool.Multiline);
                return;
            case "home.modify.move":
                ExecuteDrawingTool(DrawingTool.Move);
                return;
            case "home.modify.rotate":
                ExecuteDrawingTool(DrawingTool.Rotate);
                return;
            case "home.modify.copy":
            case "workspace.tool.copy":
                ExecuteDrawingTool(DrawingTool.Copy);
                return;
            case "home.modify.mirror":
            case "workspace.tool.mirror":
                ExecuteDrawingTool(DrawingTool.Mirror);
                return;
            case "home.draw.array":
                ExecuteArray();
                return;
            case "ribbon.herramientas.distancia":
                ExecuteDrawingTool(DrawingTool.Distance);
                return;
            case "ribbon.herramientas.angulo":
                ExecuteDrawingTool(DrawingTool.Angle);
                return;
            case "ribbon.herramientas.id-de-punto":
                ExecuteDrawingTool(DrawingTool.PointId);
                return;
            case "ribbon.herramientas.area":
                ExecuteArea();
                return;
            case "ribbon.herramientas.lista":
                ExecuteList();
                return;
            case "workspace.tool.select":
                CancelLine();
                SetPanMode(false);
                UpdateControls();
                WorkspaceViewport.Focus();
                return;
            case "document.select.drawing-1":
                WorkspaceViewport.Focus();
                return;
            case "document.close.drawing-1":
                Close();
                return;
            case "home.properties":
                SetPropertiesDockVisible(true);
                return;
            case "dock.properties.close":
                SetPropertiesDockVisible(false);
                return;
            case "home.layers.properties":
                LayerManagerPanel.IsVisible = true;
                RefreshLayerPanel();
                return;
            case "home.layers.set-current":
                ExecuteSelectedLayerMutation(
                    layer => _session.SetCurrentLayer(layer.Id),
                    layer => $"LAYER nativo - capa actual: {layer.Name}");
                return;
            case "home.layers.visible":
                ExecuteSelectedLayerMutation(
                    layer => _session.SetLayerOff(layer.Id, !layer.Off),
                    layer => $"LAYER nativo - {layer.Name}: {(layer.Off ? "visible" : "apagada")}");
                return;
            case "home.layers.lock":
                ExecuteSelectedLayerMutation(
                    layer => _session.SetLayerLocked(layer.Id, !layer.Locked),
                    layer => $"LAYER nativo - {layer.Name}: {(layer.Locked ? "desbloqueada" : "bloqueada")}");
                return;
            case "dock.layers.close":
                LayerManagerPanel.IsVisible = false;
                return;
            case "palette.aux.close":
                AuxiliaryPalettePanel.IsVisible = false;
                return;
            case "surface.close":
                TopSurfacePanel.IsVisible = false;
                return;
            case "properties.section.general":
                TogglePanel(GeneralPropertyRows, source);
                return;
            case "properties.section.view":
                TogglePanel(ViewPropertyRows, source);
                return;
            case "properties.section.misc":
                TogglePanel(MiscPropertyRows, source);
                return;
            case "status.grid":
                WorkspaceViewport.ShowGrid = !WorkspaceViewport.ShowGrid;
                SetActive(GridStatusButton, WorkspaceViewport.ShowGrid);
                return;
            case "view.ucs":
                WorkspaceViewport.ShowUcs = !WorkspaceViewport.ShowUcs;
                SetActive(UcsButton, WorkspaceViewport.ShowUcs);
                return;
            case "status.osnap":
                _objectSnapEnabled = !_objectSnapEnabled;
                _lastSnap = null;
                SetActive(OsnapStatusButton, _objectSnapEnabled);
                return;
            case "status.ortho":
                _orthoEnabled = !_orthoEnabled;
                SetActive(OrthoStatusButton, _orthoEnabled);
                return;
            case "home.annotate.dimension":
            case "workspace.tool.dimension":
            case "status.annotation":
                ToggleDimensions();
                return;
            case "status.lineweight":
                ToggleLineweight();
                return;
            case "view.nav.zoom":
                ZoomIn();
                return;
            case "view.nav.pan":
                SetPanMode(!_panMode);
                UpdateControls();
                UpdateStatus(_panMode ? "Encuadre · arrastre el dibujo" : ConnectedStatus);
                return;
            case "view.nav.view":
                FitView();
                return;
            case "view.nav.more":
                ResetView();
                return;
            case "view.viewport.controls":
                var visible = !AreViewportControlsVisible;
                ViewOrientationButton.IsVisible = visible;
                VisualStyleButton.IsVisible = visible;
                UcsButton.IsVisible = visible;
                ViewportMenuButton.Content = visible ? "[–]" : "[+]";
                SetActive(ViewportMenuButton, !visible);
                return;
            default:
                UpdateUnavailable(source);
                return;
        }
    }

    private void ShowRibbonTab(string action)
    {
        var (section, activeTab) = action switch
        {
            "ribbon.tab.inicio" => ((string?)null, HomeRibbonTab),
            "ribbon.tab.anotar" => ("Anotar", AnnotateRibbonTab),
            "ribbon.tab.insertar" => ("Insertar", InsertRibbonTab),
            "ribbon.tab.vistas" => ("Vistas", ViewsRibbonTab),
            "ribbon.tab.herramientas" => ("Herramientas", ToolsRibbonTab),
            "ribbon.tab.smart" => ("Smart", SmartRibbonTab),
            "ribbon.tab.administra" => ("Administra", ManageRibbonTab),
            "ribbon.tab.exportar" => ("Exportar", ExportRibbonTab),
            "ribbon.tab.en-linea" => ("En línea", OnlineRibbonTab),
            "ribbon.tab.geoservicio" => ("Geoservicio", GeoServiceRibbonTab),
            _ => throw new InvalidOperationException($"Pestaña de ribbon desconocida: {action}"),
        };

        foreach (var tab in new[]
        {
            HomeRibbonTab,
            AnnotateRibbonTab,
            InsertRibbonTab,
            ViewsRibbonTab,
            ToolsRibbonTab,
            SmartRibbonTab,
            ManageRibbonTab,
            ExportRibbonTab,
            OnlineRibbonTab,
            GeoServiceRibbonTab,
        })
        {
            SetActive(tab, ReferenceEquals(tab, activeTab));
        }

        RibbonInicioPanel.IsVisible = section is null;
        RibbonContextPanel.IsVisible = section is not null;
        RibbonContextPanel.ItemsSource = section is null ? null : RibbonCatalog.Sections[section];
        RibbonContextPanel.ApplyTemplate();
        RibbonContextPanel.InvalidateMeasure();
        (RibbonContextPanel.Parent as Control)?.InvalidateMeasure();
        ContextRibbonOverflowButton.IsVisible = false;
        UpdateControls();
    }

    private void ExecuteNew()
    {
        try
        {
            NewDocument();
        }
        catch (InvalidOperationException) when (_session.IsDirty)
        {
            UpdateStatus(SaveFirstStatus);
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo crear el documento - {error.Message}");
        }
    }

    private void ExecuteDrawingTool(
        DrawingTool tool,
        double parameter = 0,
        double secondaryParameter = 0,
        double tertiaryParameter = 0)
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            switch (tool)
            {
                case DrawingTool.Line:
                    StartLine();
                    break;
                case DrawingTool.Polyline:
                    StartPolyline();
                    break;
                case DrawingTool.Rectangle:
                    StartRectangle();
                    break;
                case DrawingTool.Circle:
                    StartCircle();
                    break;
                case DrawingTool.CircleTwoPoint:
                    StartCircleTwoPoint();
                    break;
                case DrawingTool.CircleThreePoint:
                    StartCircleThreePoint();
                    break;
                case DrawingTool.CircleTtr:
                    StartCircleTtr(parameter);
                    break;
                case DrawingTool.Arc:
                    StartArc();
                    break;
                case DrawingTool.ArcCenterStartEnd:
                    StartArcCenterStartEnd();
                    break;
                case DrawingTool.Ellipse:
                    StartEllipse();
                    break;
                case DrawingTool.EllipseCenter:
                    StartEllipseCenter(parameter);
                    break;
                case DrawingTool.EllipseArc:
                    StartEllipticalArc(parameter, secondaryParameter, tertiaryParameter);
                    break;
                case DrawingTool.Spline:
                    StartSpline();
                    break;
                case DrawingTool.RevisionCloud:
                    StartRevisionCloud(parameter);
                    break;
                case DrawingTool.Wipeout:
                    StartWipeout();
                    break;
                case DrawingTool.Point:
                    StartPoint();
                    break;
                case DrawingTool.Xline:
                case DrawingTool.XlineHorizontal:
                case DrawingTool.XlineVertical:
                case DrawingTool.XlineAngle:
                case DrawingTool.Ray:
                    StartTool(tool, parameter: parameter);
                    break;
                case DrawingTool.Donut:
                    StartTool(tool, parameter: parameter, secondaryParameter: secondaryParameter);
                    break;
                case DrawingTool.Multiline:
                    StartMultiline();
                    break;
                case DrawingTool.Move:
                    StartMove();
                    break;
                case DrawingTool.Rotate:
                    StartRotate();
                    break;
                case DrawingTool.Scale:
                    StartScale(parameter);
                    break;
                case DrawingTool.Offset:
                    StartOffset(parameter);
                    break;
                case DrawingTool.Trim:
                    StartTrim();
                    break;
                case DrawingTool.Extend:
                    StartExtend();
                    break;
                case DrawingTool.Chamfer:
                    StartChamfer(parameter);
                    break;
                case DrawingTool.Fillet:
                    StartFillet(parameter);
                    break;
                case DrawingTool.Break:
                    StartBreak();
                    break;
                case DrawingTool.BreakAtPoint:
                    StartBreakAtPoint();
                    break;
                case DrawingTool.Lengthen:
                    StartLengthen(parameter);
                    break;
                case DrawingTool.Stretch:
                    StartStretch();
                    break;
                case DrawingTool.Join:
                    StartJoin();
                    break;
                case DrawingTool.Align:
                    StartAlign();
                    break;
                case DrawingTool.Copy:
                    StartCopy();
                    break;
                case DrawingTool.Mirror:
                    StartMirror();
                    break;
                case DrawingTool.Distance:
                    StartDistance();
                    break;
                case DrawingTool.PointId:
                    StartPointId();
                    break;
                case DrawingTool.Angle:
                    StartAngle();
                    break;
                default:
                    throw new ArgumentOutOfRangeException(nameof(tool));
            }
            WorkspaceViewport.Focus();
        }
        catch (Exception error)
        {
            var message = $"No se pudo iniciar {ToolLabel(tool)} - {error.Message}";
            _commandSession.Fail(ToolCommandName(tool), message);
            ResetLineState();
            UpdateControls();
            UpdateStatus($"Error START_COMMAND · {message}");
        }
    }

    private void ExecuteArray()
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            CreateArray();
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo crear la matriz - {error.Message}");
        }
    }

    private void ExecuteList()
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            ListSelectedEntity();
            WorkspaceViewport.Focus();
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo ejecutar LIST - {error.Message}");
        }
    }

    private void ExecuteArea()
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            MeasureSelectedArea();
            WorkspaceViewport.Focus();
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo ejecutar AREA - {error.Message}");
        }
    }

    private void ExecuteRadius()
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            MeasureSelectedRadius();
            WorkspaceViewport.Focus();
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo medir el radio - {error.Message}");
        }
    }

    private void ExecuteLength()
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            MeasureSelectedLength();
            WorkspaceViewport.Focus();
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo medir la longitud - {error.Message}");
        }
    }

    private void ExecuteBounds()
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            MeasureSelectedBounds();
            WorkspaceViewport.Focus();
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudieron medir los limites - {error.Message}");
        }
    }

    private void ExecuteErase()
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            EraseSelectedEntity();
            WorkspaceViewport.Focus();
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo ejecutar ERASE - {error.Message}");
        }
    }

    private void ExecuteExplode()
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            ExplodeSelectedEntity();
            WorkspaceViewport.Focus();
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo ejecutar EXPLODE - {error.Message}");
        }
    }

    private void ExecuteOops()
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            RestoreLastErase();
            WorkspaceViewport.Focus();
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo ejecutar OOPS - {error.Message}");
        }
    }

    private void ExecuteOverkill()
    {
        if (IsLineActive)
        {
            return;
        }

        try
        {
            if (_session.OverkillVisible())
            {
                _undoGroups.Push(1);
                _redoGroups.Clear();
                SyncScene();
                UpdateStatus("OVERKILL nativo · duplicados eliminados");
            }
            else
            {
                UpdateControls();
                UpdateStatus("OVERKILL nativo · no se encontraron duplicados");
            }
            WorkspaceViewport.Focus();
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo ejecutar OVERKILL - {error.Message}");
        }
    }

    private async Task OpenDocumentAsync()
    {
        if (_session.IsDirty)
        {
            UpdateStatus(SaveFirstStatus);
            return;
        }

        try
        {
            if (!StorageProvider.CanOpen)
            {
                UpdateStatus("Abrir no disponible en esta plataforma");
                return;
            }

            var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
            {
                Title = "Abrir documento ArcCAD",
                AllowMultiple = false,
                FileTypeFilter = [ArcfFileType],
            });
            if (files.Count == 0)
            {
                UpdateStatus("Abrir cancelado");
                return;
            }

            var path = files[0].TryGetLocalPath();
            if (path is null)
            {
                UpdateStatus("Abrir requiere un archivo local");
                return;
            }

            OpenFromPath(path);
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo abrir - {error.Message}");
        }
    }

    private async Task SaveDocumentAsync()
    {
        try
        {
            if (_session.CurrentPath is { } currentPath)
            {
                SaveToPath(currentPath);
                return;
            }

            if (!StorageProvider.CanSave)
            {
                UpdateStatus("Guardar no disponible en esta plataforma");
                return;
            }

            var file = await StorageProvider.SaveFilePickerAsync(new FilePickerSaveOptions
            {
                Title = "Guardar documento ArcCAD",
                SuggestedFileName = "documento.arcf",
                DefaultExtension = "arcf",
                FileTypeChoices = [ArcfFileType],
            });
            if (file is null)
            {
                UpdateStatus("Guardar cancelado");
                return;
            }

            var path = file.TryGetLocalPath();
            if (path is null)
            {
                UpdateStatus("Guardar requiere un archivo local");
                return;
            }

            SaveToPath(path);
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo guardar - {error.Message}");
        }
    }

    private void OnViewportPointerPressed(object? sender, PointerPressedEventArgs eventArgs)
    {
        var current = eventArgs.GetCurrentPoint(WorkspaceViewport);
        if (current.Properties.IsMiddleButtonPressed ||
            _panMode && current.Properties.IsLeftButtonPressed)
        {
            _isPanning = true;
            _panAnchor = current.Position;
            eventArgs.Handled = true;
            return;
        }

        if (!current.Properties.IsLeftButtonPressed || eventArgs.KeyModifiers.HasFlag(KeyModifiers.Shift))
        {
            return;
        }

        var point = ToWorld(current.Position);
        try
        {
            if (IsLineActive)
            {
                AcceptPoint(point);
            }
            else
            {
                SelectAt(point);
            }
        }
        catch (Exception error)
        {
            UpdateStatus($"No se pudo completar la acción - {error.Message}");
        }

        eventArgs.Handled = true;
    }

    private void OnViewportPointerMoved(object? sender, PointerEventArgs eventArgs)
    {
        var position = eventArgs.GetPosition(WorkspaceViewport);
        if (_isPanning)
        {
            WorkspaceViewport.PanBy(position - _panAnchor);
            _panAnchor = position;
            eventArgs.Handled = true;
            return;
        }

        MovePointer(ToWorld(position));
    }

    private void OnViewportPointerReleased(object? sender, PointerReleasedEventArgs eventArgs)
    {
        if (_isPanning)
        {
            _isPanning = false;
            eventArgs.Handled = true;
        }
    }

    private void OnViewportPointerWheelChanged(object? sender, PointerWheelEventArgs eventArgs)
    {
        var factor = eventArgs.Delta.Y >= 0 ? 1.15 : 1 / 1.15;
        WorkspaceViewport.ZoomAt(factor, eventArgs.GetPosition(WorkspaceViewport));
        UpdateStatus($"Zoom · {WorkspaceViewport.Zoom:P0}");
        eventArgs.Handled = true;
    }

    private void OnWindowKeyDown(object? sender, KeyEventArgs eventArgs)
    {
        if (HandleKey(eventArgs.Key, eventArgs.KeyModifiers))
        {
            eventArgs.Handled = true;
        }
    }

    public bool ExecuteFirstCommandOption()
    {
        var option = _commandSession.Options.FirstOrDefault(candidate =>
            !string.Equals(candidate, _commandSession.DefaultOption, StringComparison.Ordinal));
        option ??= _commandSession.DefaultOption;
        if (option is null)
        {
            UpdateStatus("El comando activo no tiene opciones");
            return false;
        }

        SubmitCommand(option);
        return true;
    }

    private bool TryFinishActiveCommand(string command)
    {
        if (!_commandSession.IsActive || command != "FINISH")
        {
            return false;
        }

        switch (_drawingTool)
        {
            case DrawingTool.Polyline when _pathPoints.Count >= 2:
                FinishPolyline();
                return true;
            case DrawingTool.Spline when _pathPoints.Count >= 3:
                FinishSpline();
                return true;
            case DrawingTool.Wipeout when _pathPoints.Count >= 3:
                FinishWipeout();
                return true;
            default:
                var message = $"{ToolCommandName(_drawingTool)} necesita mas puntos antes de confirmar";
                _commandSession.RejectInput(message);
                UpdateStatus($"Error INCOMPLETE_COMMAND · {message}");
                return true;
        }
    }

    private bool TryExecuteActivePolylineCommand(string? command)
    {
        if (_drawingTool != DrawingTool.Polyline || command is not ("C" or "CLOSE"))
        {
            return false;
        }

        if (_pathPoints.Count < 3)
        {
            const string message = "PLINE · CLOSE requiere al menos 3 vértices";
            _commandSession.RejectInput(message);
            UpdateStatus($"Error INCOMPLETE_COMMAND · {message}");
            return true;
        }

        FinishPolyline(closed: true);
        return true;
    }

    private void OnCommandInputKeyDown(object? sender, KeyEventArgs eventArgs)
    {
        if (eventArgs.Key != Key.Enter)
        {
            return;
        }

        var input = CommandInput.Text;
        CommandInput.Text = null;
        SubmitCommand(input);
        eventArgs.Handled = true;
    }

    public void SubmitCommand(string? input)
    {
        var resolveInitialAlias = !_commandSession.IsActive;
        var rawCommandInput = input;
        string command;
        try
        {
            command = _commandSession.ResolveInput(input);
        }
        catch (CommandInputException error)
        {
            if (_commandSession.IsActive)
            {
                _commandSession.RejectInput(error.Message);
            }
            UpdateStatus($"Error {error.Code} · {error.Message}");
            return;
        }

        if (TryFinishActiveCommand(command))
        {
            return;
        }

        if (TryExecuteActivePolylineCommand(command))
        {
            return;
        }

        if (resolveInitialAlias)
        {
            try
            {
                (command, rawCommandInput) = ResolveInitialCommandAlias(command, rawCommandInput);
            }
            catch (Exception error)
            {
                var failedCommand = FirstCommandToken(command);
                var message = $"No se pudo resolver el alias '{failedCommand}': {error.Message}";
                _commandSession.Fail(failedCommand, message);
                UpdateStatus($"Error PGP_RESOLVE - {message}");
                return;
            }

            if (TryExecuteAliasAdministration(rawCommandInput, command))
            {
                return;
            }
        }

        if (TryExecuteXlineCommand(command))
        {
            return;
        }

        if (TryExecuteDonutCommand(command))
        {
            return;
        }

        if (TryExecuteRectangleCommand(command))
        {
            return;
        }

        if (TryExecutePolygonCommand(command))
        {
            return;
        }

        if (TryExecuteNudgeCommand(command))
        {
            return;
        }

        if (TryExecuteCircleModeCommand(command))
        {
            return;
        }

        if (TryExecuteArcModeCommand(command))
        {
            return;
        }

        if (TryExecuteEllipseModeCommand(command))
        {
            return;
        }

        if (TryExecuteLayerCommand(rawCommandInput, command))
        {
            return;
        }

        if (TryExecuteRevisionCloudCommand(command))
        {
            return;
        }

        if (TryExecuteParameterizedCommand(command))
        {
            return;
        }

        switch (command)
        {
            case "LINE":
            case "L":
                ExecuteDrawingTool(DrawingTool.Line);
                break;
            case "PLINE":
            case "POLYLINE":
            case "PL":
                ExecuteDrawingTool(DrawingTool.Polyline);
                break;
            case "RECTANGLE":
            case "RECTANG":
            case "REC":
                ExecuteDrawingTool(DrawingTool.Rectangle);
                break;
            case "POLYGON":
            case "POL":
                UpdateStatus("Uso: POLYGON <lados> [I|C], por ejemplo POLYGON 6");
                break;
            case "CIRCLE":
            case "C":
                ExecuteDrawingTool(DrawingTool.Circle);
                break;
            case "ARC":
            case "A":
                ExecuteDrawingTool(DrawingTool.Arc);
                break;
            case "ELLIPSE":
            case "EL":
                ExecuteDrawingTool(DrawingTool.Ellipse);
                break;
            case "SPLINE":
            case "SPL":
                ExecuteDrawingTool(DrawingTool.Spline);
                break;
            case "REVCLOUD":
                UpdateStatus("Uso: REVCLOUD <longitud de arco>, por ejemplo REVCLOUD 10");
                break;
            case "WIPEOUT":
                ExecuteDrawingTool(DrawingTool.Wipeout);
                break;
            case "POINT":
            case "PO":
                ExecuteDrawingTool(DrawingTool.Point);
                break;
            case "RAY":
                ExecuteDrawingTool(DrawingTool.Ray);
                break;
            case "MULTILINE":
            case "ML":
                ExecuteDrawingTool(DrawingTool.Multiline);
                break;
            case "MOVE":
            case "M":
                ExecuteDrawingTool(DrawingTool.Move);
                break;
            case "ROTATE":
            case "RO":
                ExecuteDrawingTool(DrawingTool.Rotate);
                break;
            case "SCALE":
            case "SC":
                UpdateStatus("Uso: SCALE <factor>, por ejemplo SCALE 2");
                break;
            case "OFFSET":
            case "O":
                UpdateStatus("Uso: OFFSET <distancia>, por ejemplo OFFSET 3");
                break;
            case "TRIM":
            case "TR":
                ExecuteDrawingTool(DrawingTool.Trim);
                break;
            case "EXTEND":
            case "EX":
                ExecuteDrawingTool(DrawingTool.Extend);
                break;
            case "CHAMFER":
            case "CHA":
                UpdateStatus("Uso: CHAMFER <distancia>, por ejemplo CHA 2");
                break;
            case "FILLET":
            case "F":
                UpdateStatus("Uso: FILLET <radio>, por ejemplo F 10");
                break;
            case "BREAK":
            case "BR":
                ExecuteDrawingTool(DrawingTool.Break);
                break;
            case "BREAKATPOINT":
                ExecuteDrawingTool(DrawingTool.BreakAtPoint);
                break;
            case "LENGTHEN":
            case "LEN":
                UpdateStatus("Uso: LENGTHEN <longitud total>, por ejemplo LEN 15");
                break;
            case "STRETCH":
            case "S":
                ExecuteDrawingTool(DrawingTool.Stretch);
                break;
            case "JOIN":
            case "J":
                ExecuteDrawingTool(DrawingTool.Join);
                break;
            case "ALIGN":
            case "AL":
                ExecuteDrawingTool(DrawingTool.Align);
                break;
            case "NUDGE":
                UpdateStatus("Uso: NUDGE <dx> <dy>, por ejemplo NUDGE 2 -3");
                break;
            case "OVERKILL":
                ExecuteOverkill();
                break;
            case "COPY":
            case "CO":
                ExecuteDrawingTool(DrawingTool.Copy);
                break;
            case "MIRROR":
            case "MI":
                ExecuteDrawingTool(DrawingTool.Mirror);
                break;
            case "ARRAY":
                ExecuteArray();
                break;
            case "DIST":
            case "DI":
                ExecuteDrawingTool(DrawingTool.Distance);
                break;
            case "ID":
                ExecuteDrawingTool(DrawingTool.PointId);
                break;
            case "ANGLE":
                ExecuteDrawingTool(DrawingTool.Angle);
                break;
            case "AREA":
            case "AA":
                ExecuteArea();
                break;
            case "MEASUREGEOM":
            case "MEA":
                UpdateStatus("Uso: MEASUREGEOM RADIUS|LENGTH|BOUNDS");
                break;
            case "LIST":
            case "LI":
                ExecuteList();
                break;
            case "ERASE":
            case "E":
                ExecuteErase();
                break;
            case "EXPLODE":
            case "X":
                ExecuteExplode();
                break;
            case "OOPS":
                ExecuteOops();
                break;
            case "DIM":
                ToggleDimensions();
                break;
            case "ZOOMIN":
                ZoomIn();
                break;
            case "ZOOMOUT":
                ZoomOut();
                break;
            case "FIT":
                FitView();
                break;
            case "PAN":
                SetPanMode(!_panMode);
                UpdateControls();
                UpdateStatus(_panMode ? "Encuadre · arrastre el dibujo" : ConnectedStatus);
                break;
            default:
                var message = $"Comando desconocido: {command}";
                _commandSession.Fail(command, message);
                UpdateStatus($"Error UNKNOWN_COMMAND · {message}");
                break;
        }
    }

    private (string Command, string RawInput) ResolveInitialCommandAlias(
        string command,
        string? rawInput)
    {
        var token = FirstCommandToken(command);
        var canonical = _session.ResolveCommandAlias(token);
        var normalizedSuffix = command[token.Length..];
        var raw = string.IsNullOrWhiteSpace(rawInput) ? command : rawInput.Trim();
        var rawToken = FirstCommandToken(raw);
        return (canonical + normalizedSuffix, canonical + raw[rawToken.Length..]);
    }

    private bool TryExecuteAliasAdministration(string? rawInput, string command)
    {
        var commandName = FirstCommandToken(command);
        if (commandName is not ("REINIT" or "ALIASEDIT"))
        {
            return false;
        }

        try
        {
            _commandSession.Begin(commandName, $"{commandName} en curso");
            string status;
            if (commandName == "REINIT")
            {
                if (CommandSuffix(command).Length != 0)
                {
                    throw new ArgumentException("Uso: REINIT");
                }

                status = $"REINIT - {CompactQueryResult(_session.ReloadAliases())}";
            }
            else
            {
                status = ExecuteAliasEdit(rawInput ?? command);
            }

            _commandSession.Complete(status);
            UpdateStatus(status);
        }
        catch (Exception error)
        {
            var message = $"{commandName} - {error.Message}";
            _commandSession.Fail(commandName, message);
            UpdateStatus($"Error {message}");
        }

        return true;
    }

    private string ExecuteAliasEdit(string rawInput)
    {
        var payload = CommandSuffix(rawInput);
        if (payload.Length == 0)
        {
            _session.EnsureAliasFile();
            return $"ALIASEDIT - {_session.AliasFilePath}";
        }

        var fields = payload.Split(' ', 2, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (fields[0].Equals("REMOVE", StringComparison.OrdinalIgnoreCase))
        {
            if (fields.Length != 2 || !IsAliasToken(fields[1]))
            {
                throw new ArgumentException("Uso: ALIASEDIT REMOVE <alias>");
            }

            var removedContent = RemoveAliasDefinitions(_session.AliasContent, fields[1]);
            var removedResult = _session.SaveAliases(removedContent);
            return $"ALIASEDIT - eliminado {fields[1]} - {CompactQueryResult(removedResult)}";
        }

        var comma = payload.IndexOf(',');
        if (comma <= 0)
        {
            throw new ArgumentException("Uso: ALIASEDIT <alias>,*<comando>");
        }

        var alias = payload[..comma].Trim();
        var targetField = payload[(comma + 1)..].Trim();
        if (!targetField.StartsWith('*'))
        {
            throw new ArgumentException("Uso: ALIASEDIT <alias>,*<comando>");
        }

        var target = targetField[1..].Trim();
        if (!IsAliasToken(alias) || !IsAliasToken(target))
        {
            throw new ArgumentException("Alias y comando deben ser tokens no vacios sin espacios ni comas.");
        }

        var canonicalTarget = _session.ResolveKnownCommand(target)
            ?? throw new ArgumentException($"Comando destino desconocido: {target}");

        var contentWithoutAlias = RemoveAliasDefinitions(_session.AliasContent, alias);
        var separator = contentWithoutAlias.Length == 0 ||
            contentWithoutAlias.EndsWith('\n') || contentWithoutAlias.EndsWith('\r')
                ? string.Empty
                : Environment.NewLine;
        var content = $"{contentWithoutAlias}{separator}{alias},*{canonicalTarget}{Environment.NewLine}";
        var result = _session.SaveAliases(content);
        return $"ALIASEDIT - {alias} -> {canonicalTarget} - {CompactQueryResult(result)}";
    }

    private static string RemoveAliasDefinitions(string content, string alias)
    {
        var normalized = content.Replace("\r\n", "\n", StringComparison.Ordinal).Replace('\r', '\n');
        var lines = normalized.Split('\n');
        return string.Join(
            Environment.NewLine,
            lines.Where(line => !DefinesAlias(line, alias)));
    }

    private static bool DefinesAlias(string rawLine, string alias)
    {
        var line = rawLine.Trim();
        if (line.Length == 0 || line.StartsWith(';'))
        {
            return false;
        }

        var comma = line.IndexOf(',');
        return comma > 0 &&
            line[(comma + 1)..].TrimStart().StartsWith('*') &&
            AliasComparisonKey(line[..comma].Trim()) == AliasComparisonKey(alias);
    }

    private static string AliasComparisonKey(string value) =>
        value
            .Normalize(System.Text.NormalizationForm.FormKC)
            .Replace("ß", "ss", StringComparison.Ordinal)
            .Replace("ẞ", "ss", StringComparison.Ordinal)
            .ToUpperInvariant();

    private static bool IsAliasToken(string value) =>
        !string.IsNullOrWhiteSpace(value) &&
        !value.StartsWith(';') &&
        !value.Contains(',') &&
        !value.Any(char.IsWhiteSpace);

    private static string FirstCommandToken(string value)
    {
        var index = value.IndexOfAny([' ', '\t', '\r', '\n']);
        return index < 0 ? value : value[..index];
    }

    private static string CommandSuffix(string value)
    {
        var index = value.IndexOfAny([' ', '\t', '\r', '\n']);
        return index < 0 ? string.Empty : value[(index + 1)..].Trim();
    }

    private bool TryExecuteRectangleCommand(string? command)
    {
        if (string.IsNullOrWhiteSpace(command))
        {
            return false;
        }

        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts[0] is not ("RECTANGLE" or "RECTANG" or "REC"))
        {
            return false;
        }

        double? chamfer1 = null;
        double? chamfer2 = null;
        double? fillet = null;
        double? width = null;
        var valid = true;
        for (var index = 1; valid && index < parts.Length;)
        {
            switch (parts[index])
            {
                case "CHAMFER" when chamfer1 is null && fillet is null && index + 2 < parts.Length:
                    {
                        var first = 0.0;
                        var second = 0.0;
                        valid = TryParsePositiveRectangleDistance(parts[index + 1], out first) &&
                            TryParsePositiveRectangleDistance(parts[index + 2], out second);
                        if (valid)
                        {
                            chamfer1 = first;
                            chamfer2 = second;
                            index += 3;
                        }
                        break;
                    }
                case "FILLET" when chamfer1 is null && fillet is null && index + 1 < parts.Length:
                    valid = TryParsePositiveRectangleDistance(parts[index + 1], out var radius);
                    if (valid)
                    {
                        fillet = radius;
                        index += 2;
                    }
                    break;
                case "WIDTH" when width is null && index + 1 < parts.Length:
                    valid = TryParsePositiveRectangleDistance(parts[index + 1], out var strokeWidth);
                    if (valid)
                    {
                        width = strokeWidth;
                        index += 2;
                    }
                    break;
                default:
                    valid = false;
                    break;
            }
        }

        if (!valid)
        {
            UpdateStatus("Parámetros inválidos · uso: RECTANG [CHAMFER d1 d2 | FILLET r] [WIDTH w]");
            return true;
        }

        if (!IsLineActive)
        {
            try
            {
                StartRectangle(chamfer1, chamfer2, fillet, width);
                WorkspaceViewport.Focus();
            }
            catch (Exception error)
            {
                UpdateStatus($"No se pudo iniciar RECTANG - {error.Message}");
            }
        }

        return true;
    }

    private static bool TryParsePositiveRectangleDistance(string text, out double value) =>
        double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out value) &&
        double.IsFinite(value) && value > 0.0;

    private bool TryExecutePolygonCommand(string? command)
    {
        if (string.IsNullOrWhiteSpace(command))
        {
            return false;
        }

        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length is < 2 or > 3 || parts[0] is not ("POLYGON" or "POL"))
        {
            return false;
        }

        var modeValid = parts.Length == 2 || parts[2] is "I" or "INSCRIBED" or "C" or "CIRCUMSCRIBED";
        if (!int.TryParse(parts[1], NumberStyles.None, CultureInfo.InvariantCulture, out var sides) ||
            sides is < 3 or > 1024 || !modeValid)
        {
            UpdateStatus("Parámetros inválidos · uso: POLYGON <3..1024> [I|C]");
            return true;
        }

        if (!IsLineActive)
        {
            try
            {
                StartPolygon(sides, parts.Length == 3 && parts[2] is "C" or "CIRCUMSCRIBED");
                WorkspaceViewport.Focus();
            }
            catch (Exception error)
            {
                UpdateStatus($"No se pudo iniciar POLYGON - {error.Message}");
            }
        }

        return true;
    }

    private bool TryExecuteNudgeCommand(string? command)
    {
        if (string.IsNullOrWhiteSpace(command))
        {
            return false;
        }

        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length != 3 || parts[0] != "NUDGE")
        {
            return false;
        }

        if (!double.TryParse(parts[1], NumberStyles.Float, CultureInfo.InvariantCulture, out var dx) ||
            !double.TryParse(parts[2], NumberStyles.Float, CultureInfo.InvariantCulture, out var dy) ||
            !double.IsFinite(dx) || !double.IsFinite(dy) || dx == 0 && dy == 0)
        {
            UpdateStatus("Vector inválido para NUDGE");
            return true;
        }

        if (!IsLineActive)
        {
            try
            {
                CompleteNativeMutation(
                    () => _session.NudgeSelected(new ArcCadPoint(dx, dy)),
                    $"NUDGE nativo · Δ({dx.ToString("G", CultureInfo.InvariantCulture)}, {dy.ToString("G", CultureInfo.InvariantCulture)})");
            }
            catch (Exception error)
            {
                UpdateStatus($"No se pudo ejecutar NUDGE - {error.Message}");
            }
        }

        return true;
    }

    private bool TryExecuteDonutCommand(string? command)
    {
        if (string.IsNullOrWhiteSpace(command))
        {
            return false;
        }

        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts[0] is not ("DONUT" or "DO"))
        {
            return false;
        }

        var validCount = parts.Length is 2 or 3;
        var interiorText = parts.Length == 3 ? parts[1] : "0";
        var exteriorText = parts.Length == 3 ? parts[2] : parts.ElementAtOrDefault(1);
        if (!validCount ||
            !double.TryParse(interiorText, NumberStyles.Float, CultureInfo.InvariantCulture, out var interior) ||
            !double.TryParse(exteriorText, NumberStyles.Float, CultureInfo.InvariantCulture, out var exterior) ||
            !double.IsFinite(interior) || !double.IsFinite(exterior) ||
            interior < 0 || exterior <= 0 || interior >= exterior)
        {
            UpdateStatus("Uso: DONUT <exterior> o DONUT <interior> <exterior>");
            return true;
        }

        ExecuteDrawingTool(DrawingTool.Donut, exterior, interior);
        return true;
    }

    private bool TryExecuteXlineCommand(string? command)
    {
        if (string.IsNullOrWhiteSpace(command))
        {
            return false;
        }

        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts[0] is not ("XLINE" or "XL"))
        {
            return false;
        }

        if (parts.Length == 1)
        {
            ExecuteDrawingTool(DrawingTool.Xline);
            return true;
        }

        if (parts.Length == 2 && parts[1] is "H" or "HOR")
        {
            ExecuteDrawingTool(DrawingTool.XlineHorizontal);
            return true;
        }

        if (parts.Length == 2 && parts[1] is "V" or "VER")
        {
            ExecuteDrawingTool(DrawingTool.XlineVertical);
            return true;
        }

        if (parts.Length == 3 && parts[1] is "A" or "ANG" &&
            double.TryParse(parts[2], NumberStyles.Float, CultureInfo.InvariantCulture, out var degrees) &&
            double.IsFinite(degrees))
        {
            ExecuteDrawingTool(DrawingTool.XlineAngle, degrees * Math.PI / 180);
            return true;
        }

        UpdateStatus("Uso: XLINE [H|V|A <grados>]");
        return true;
    }

    private bool TryExecuteLayerCommand(string? rawInput, string command)
    {
        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length == 0 || parts[0] is not ("LAYER" or "LA"))
        {
            return false;
        }

        var rawParts = (rawInput ?? command)
            .Trim()
            .Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        try
        {
            if (parts.Length == 1)
            {
                LayerManagerPanel.IsVisible = true;
                RefreshLayerPanel();
                CompleteLayerQuery("LAYER - use LIST, NEW, DELETE, RENAME, SET-CURRENT, ON, OFF, FREEZE, THAW, LOCK, UNLOCK, PLOT o NO-PLOT");
                return true;
            }

            switch (parts[1])
            {
                case "LIST" when parts.Length == 2:
                    var layers = _session.Layers;
                    CompleteLayerQuery(
                        $"LAYER - {layers.Count} capas - " +
                        string.Join(", ", layers.Select(layer => $"{layer.Id}:{layer.Name}{(layer.Current ? "*" : string.Empty)}")));
                    return true;
                case "NEW" when rawParts.Length >= 3:
                    var newName = string.Join(' ', rawParts.Skip(2));
                    CompleteNativeMutation(
                        () =>
                        {
                            _session.CreateLayer(newName);
                            _selectedLayerId = _session.ResolveLayer(newName).Id;
                        },
                        $"LAYER nativo - creada: {newName}");
                    return true;
                case "DELETE" when rawParts.Length == 3:
                    var deleted = _session.ResolveLayer(rawParts[2]);
                    CompleteNativeMutation(
                        () =>
                        {
                            _session.DeleteLayer(deleted.Id);
                            _selectedLayerId = null;
                        },
                        $"LAYER nativo - eliminada: {deleted.Name}");
                    return true;
                case "RENAME" when rawParts.Length >= 4:
                    var renamed = _session.ResolveLayer(rawParts[2]);
                    var renamedName = string.Join(' ', rawParts.Skip(3));
                    CompleteNativeMutation(
                        () => _session.RenameLayer(renamed.Id, renamedName),
                        $"LAYER nativo - {renamed.Name} renombrada a {renamedName}");
                    _selectedLayerId = renamed.Id;
                    RefreshLayerPanel();
                    return true;
                case "SET-CURRENT" when rawParts.Length == 3:
                    return ExecuteLayerCommandMutation(
                        rawParts[2],
                        layer => _session.SetCurrentLayer(layer.Id),
                        layer => $"LAYER nativo - capa actual: {layer.Name}");
                case "ON" when rawParts.Length == 3:
                    return ExecuteLayerCommandMutation(
                        rawParts[2],
                        layer => _session.SetLayerOff(layer.Id, false),
                        layer => $"LAYER nativo - encendida: {layer.Name}");
                case "OFF" when rawParts.Length == 3:
                    return ExecuteLayerCommandMutation(
                        rawParts[2],
                        layer => _session.SetLayerOff(layer.Id, true),
                        layer => $"LAYER nativo - apagada: {layer.Name}");
                case "FREEZE" when rawParts.Length == 3:
                    return ExecuteLayerCommandMutation(
                        rawParts[2],
                        layer => _session.SetLayerFrozen(layer.Id, true),
                        layer => $"LAYER nativo - inutilizada: {layer.Name}");
                case "THAW" when rawParts.Length == 3:
                    return ExecuteLayerCommandMutation(
                        rawParts[2],
                        layer => _session.SetLayerFrozen(layer.Id, false),
                        layer => $"LAYER nativo - reutilizada: {layer.Name}");
                case "LOCK" when rawParts.Length == 3:
                    return ExecuteLayerCommandMutation(
                        rawParts[2],
                        layer => _session.SetLayerLocked(layer.Id, true),
                        layer => $"LAYER nativo - bloqueada: {layer.Name}");
                case "UNLOCK" when rawParts.Length == 3:
                    return ExecuteLayerCommandMutation(
                        rawParts[2],
                        layer => _session.SetLayerLocked(layer.Id, false),
                        layer => $"LAYER nativo - desbloqueada: {layer.Name}");
                case "PLOT" when rawParts.Length == 3:
                    return ExecuteLayerCommandMutation(
                        rawParts[2],
                        layer => _session.SetLayerPlot(layer.Id, true),
                        layer => $"LAYER nativo - trazable: {layer.Name}");
                case "NO-PLOT" when rawParts.Length == 3:
                    return ExecuteLayerCommandMutation(
                        rawParts[2],
                        layer => _session.SetLayerPlot(layer.Id, false),
                        layer => $"LAYER nativo - no trazable: {layer.Name}");
                default:
                    throw new ArgumentException(
                        "Uso: LAYER LIST | NEW <nombre> | DELETE <id|nombre> | RENAME <id|nombre> <nombre> | " +
                        "SET-CURRENT|ON|OFF|FREEZE|THAW|LOCK|UNLOCK|PLOT|NO-PLOT <id|nombre>");
            }
        }
        catch (Exception error)
        {
            if (_commandSession.IsActive)
            {
                _commandSession.Fail(command, error.Message);
            }

            RefreshLayerPanel();
            UpdateStatus($"Error LAYER - {error.Message}");
            return true;
        }
    }

    private bool ExecuteLayerCommandMutation(
        string reference,
        Action<ArcCadLayerInfo> mutation,
        Func<ArcCadLayerInfo, string> status)
    {
        var layer = _session.ResolveLayer(reference);
        _selectedLayerId = layer.Id;
        CompleteNativeMutation(() => mutation(layer), status(layer));
        return true;
    }

    private void CompleteLayerQuery(string status)
    {
        if (_commandSession.IsActive)
        {
            _commandSession.Complete(status);
        }

        UpdateStatus(status);
    }

    private void ExecuteLayerUiAction(string action)
    {
        try
        {
            var parts = action.Split('.', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
            var operation = parts.ElementAtOrDefault(1);
            if (operation == "new")
            {
                var name = LayerSearchBox.Text?.Trim();
                ArgumentException.ThrowIfNullOrWhiteSpace(name);
                CompleteNativeMutation(
                    () =>
                    {
                        _session.CreateLayer(name);
                        _selectedLayerId = _session.ResolveLayer(name).Id;
                    },
                    $"LAYER nativo - creada: {name}");
                return;
            }

            if (operation == "select" && TryParseLayerActionId(parts, out var selectedId))
            {
                _selectedLayerId = _session.ResolveLayer(selectedId.ToString(CultureInfo.InvariantCulture)).Id;
                RefreshLayerPanel();
                UpdateStatus($"Capa seleccionada: {SelectedLayer().Name}");
                return;
            }

            var layer = SelectedLayer();
            switch (operation)
            {
                case "delete":
                    CompleteNativeMutation(
                        () =>
                        {
                            _session.DeleteLayer(layer.Id);
                            _selectedLayerId = null;
                        },
                        $"LAYER nativo - eliminada: {layer.Name}");
                    return;
                case "rename":
                    var name = LayerSearchBox.Text?.Trim();
                    ArgumentException.ThrowIfNullOrWhiteSpace(name);
                    CompleteNativeMutation(
                        () => _session.RenameLayer(layer.Id, name),
                        $"LAYER nativo - {layer.Name} renombrada a {name}");
                    return;
                case "visible":
                    CompleteNativeMutation(
                        () => _session.SetLayerOff(layer.Id, !layer.Off),
                        $"LAYER nativo - {layer.Name}: {(layer.Off ? "visible" : "apagada")}");
                    return;
                case "lock":
                    CompleteNativeMutation(
                        () => _session.SetLayerLocked(layer.Id, !layer.Locked),
                        $"LAYER nativo - {layer.Name}: {(layer.Locked ? "desbloqueada" : "bloqueada")}");
                    return;
                case "plot":
                    CompleteNativeMutation(
                        () => _session.SetLayerPlot(layer.Id, !layer.Plot),
                        $"LAYER nativo - {layer.Name}: {(layer.Plot ? "no trazable" : "trazable")}");
                    return;
                case "freeze":
                    CompleteNativeMutation(
                        () => _session.SetLayerFrozen(layer.Id, !layer.Frozen),
                        $"LAYER nativo - {layer.Name}: {(layer.Frozen ? "reutilizada" : "inutilizada")}");
                    return;
                default:
                    UpdateUnavailable("control de capas");
                    return;
            }
        }
        catch (Exception error)
        {
            RefreshLayerPanel();
            UpdateStatus($"No se pudo ejecutar LAYER - {error.Message}");
        }
    }

    private void ExecuteSelectedLayerMutation(
        Action<ArcCadLayerInfo> mutation,
        Func<ArcCadLayerInfo, string> status)
    {
        try
        {
            var layer = SelectedLayer();
            CompleteNativeMutation(() => mutation(layer), status(layer));
        }
        catch (Exception error)
        {
            RefreshLayerPanel();
            UpdateStatus($"No se pudo ejecutar LAYER - {error.Message}");
        }
    }

    private ArcCadLayerInfo SelectedLayer()
    {
        var layers = _session.Layers;
        var selected = _selectedLayerId is { } selectedId
            ? layers.SingleOrDefault(layer => layer.Id == selectedId)
            : default;
        if (selected.Id == 0)
        {
            selected = layers.Single(layer => layer.Current);
            _selectedLayerId = selected.Id;
        }

        return selected;
    }

    private static bool TryParseLayerActionId(string[] parts, out ulong layerId)
    {
        layerId = 0;
        return parts.Length == 3 &&
            ulong.TryParse(parts[2], NumberStyles.None, CultureInfo.InvariantCulture, out layerId) &&
            layerId != 0;
    }

    private void RefreshLayerPanel()
    {
        var layers = _session.Layers;
        var current = layers.Single(layer => layer.Current);
        if (_selectedLayerId is not { } selectedId || layers.All(layer => layer.Id != selectedId))
        {
            _selectedLayerId = current.Id;
        }

        _refreshingLayerUi = true;
        try
        {
            CurrentLayerCombo.ItemsSource = layers.Select(layer => layer.Name).ToArray();
            CurrentLayerCombo.SelectedIndex = layers
                .Select((layer, index) => (layer, index))
                .Single(item => item.layer.Id == current.Id)
                .index;
            CurrentLayerText.Text = current.Name;
            LayerRowsList.Children.Clear();
            foreach (var layer in layers)
            {
                LayerRowsList.Children.Add(CreateLayerRow(layer));
            }

            LayerPanelStatus.Text = $"{layers.Count} capas - capa actual: {current.Name}";
        }
        finally
        {
            _refreshingLayerUi = false;
        }
    }

    private Grid CreateLayerRow(ArcCadLayerInfo layer)
    {
        var row = new Grid
        {
            ColumnDefinitions = new ColumnDefinitions("32,32,*,70,70,54,38,38"),
            Margin = new Thickness(8, 2),
            MinHeight = 28,
        };
        row.Classes.Add("layerRow");
        if (layer.Id == _selectedLayerId)
        {
            row.Classes.Add("selected");
        }

        AddLayerCell(row, CreateLayerButton(layer.Off ? "-" : "V", $"layers.visible.{layer.Id}", $"Visibilidad de {layer.Name}", !layer.Off), 0);
        AddLayerCell(row, CreateLayerButton(layer.Locked ? "L" : "-", $"layers.lock.{layer.Id}", $"Bloqueo de {layer.Name}", layer.Locked), 1);
        AddLayerCell(row, CreateLayerButton(layer.Current ? $"* {layer.Name}" : layer.Name, $"layers.select.{layer.Id}", $"Seleccionar {layer.Name}", layer.Current), 2);
        AddLayerCell(row, new TextBlock { Text = "PorCapa" }, 3);
        AddLayerCell(row, new TextBlock { Text = "Continua" }, 4);
        AddLayerCell(row, new TextBlock { Text = "Predet." }, 5);
        AddLayerCell(row, CreateLayerButton(layer.Plot ? "P" : "-", $"layers.plot.{layer.Id}", $"Trazado de {layer.Name}", layer.Plot), 6);
        AddLayerCell(row, CreateLayerButton(layer.Frozen ? "F" : "-", $"layers.freeze.{layer.Id}", $"Inutilizar {layer.Name}", layer.Frozen), 7);
        return row;
    }

    private Button CreateLayerButton(string content, string tag, string accessibleName, bool active)
    {
        var button = new Button
        {
            Content = content,
            Tag = tag,
        };
        button.Classes.Add("layerToggle");
        if (active)
        {
            button.Classes.Add("active");
        }

        button.Click += OnUiActionClick;
        AutomationProperties.SetName(button, accessibleName);
        return button;
    }

    private static void AddLayerCell(Grid row, Control control, int column)
    {
        Grid.SetColumn(control, column);
        row.Children.Add(control);
    }

    private bool TryExecuteRevisionCloudCommand(string? command)
    {
        if (string.IsNullOrWhiteSpace(command))
        {
            return false;
        }

        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts[0] is not ("REVCLOUD" or "RC"))
        {
            return false;
        }

        var convert = parts.Length > 1 && parts[1] == "CONVERT";
        var valueIndex = convert ? 2 : 1;
        if (parts.Length < valueIndex + 1 || parts.Length > valueIndex + 2 ||
            !double.TryParse(parts[valueIndex], NumberStyles.Float, CultureInfo.InvariantCulture, out var arcLength) ||
            !double.IsFinite(arcLength) || arcLength <= 0)
        {
            UpdateStatus("Uso: REVCLOUD|RC <arco> [NORMAL|CALLIGRAPHY] o REVCLOUD|RC CONVERT <arco> [estilo]");
            return true;
        }

        var style = parts.Length == valueIndex + 2
            ? parts[valueIndex + 1].ToUpperInvariant()
            : "NORMAL";
        if (style is not ("NORMAL" or "CALLIGRAPHY"))
        {
            UpdateStatus("REVCLOUD requiere estilo NORMAL o CALLIGRAPHY");
            return true;
        }

        if (convert)
        {
            CompleteNativeMutation(
                () => _session.ConvertRevisionCloudSelected(arcLength, style),
                $"REVCLOUD CONVERT {style} - una polilinea reemplazada");
        }
        else
        {
            StartRevisionCloud(arcLength, style);
        }

        return true;
    }

    private bool TryExecuteParameterizedCommand(string? command)
    {
        if (string.IsNullOrWhiteSpace(command))
        {
            return false;
        }

        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length != 2)
        {
            return false;
        }

        if (parts[0] is "MEASUREGEOM" or "MEA")
        {
            if (parts[1] == "RADIUS")
            {
                ExecuteRadius();
            }
            else if (parts[1] == "LENGTH")
            {
                ExecuteLength();
            }
            else if (parts[1] == "BOUNDS")
            {
                ExecuteBounds();
            }
            else
            {
                UpdateStatus("Uso: MEASUREGEOM RADIUS|LENGTH|BOUNDS");
            }

            return true;
        }

        var tool = parts[0] switch
        {
            "SCALE" or "SC" => DrawingTool.Scale,
            "OFFSET" or "O" => DrawingTool.Offset,
            "CHAMFER" or "CHA" => DrawingTool.Chamfer,
            "FILLET" or "F" => DrawingTool.Fillet,
            "LENGTHEN" or "LEN" => DrawingTool.Lengthen,
            "REVCLOUD" => DrawingTool.RevisionCloud,
            _ => DrawingTool.None,
        };
        if (tool == DrawingTool.None)
        {
            return false;
        }

        if (!double.TryParse(parts[1], NumberStyles.Float, CultureInfo.InvariantCulture, out var value) ||
            !double.IsFinite(value) || value <= 0 ||
            tool == DrawingTool.Scale && value == 1)
        {
            UpdateStatus($"Parámetro inválido para {parts[0]}");
            return true;
        }

        ExecuteDrawingTool(tool, value);
        return true;
    }

    private bool TryExecuteCircleModeCommand(string? command)
    {
        if (string.IsNullOrWhiteSpace(command))
        {
            return false;
        }

        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length < 2 || parts[0] is not ("CIRCLE" or "C"))
        {
            return false;
        }

        if (parts.Length == 2 && parts[1] is "2P" or "3P")
        {
            ExecuteDrawingTool(parts[1] == "2P" ? DrawingTool.CircleTwoPoint : DrawingTool.CircleThreePoint);
            return true;
        }

        if (parts[1] != "TTR")
        {
            return false;
        }

        if (parts.Length != 3 ||
            !double.TryParse(parts[2], NumberStyles.Float, CultureInfo.InvariantCulture, out var radius) ||
            !double.IsFinite(radius) || radius <= 0)
        {
            UpdateStatus("Uso: CIRCLE TTR <radio>, por ejemplo CIRCLE TTR 10");
            return true;
        }

        ExecuteDrawingTool(DrawingTool.CircleTtr, radius);
        return true;
    }

    private bool TryExecuteArcModeCommand(string? command)
    {
        if (string.IsNullOrWhiteSpace(command))
        {
            return false;
        }

        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length != 2 || parts[0] is not ("ARC" or "A") || parts[1] != "CSE")
        {
            return false;
        }

        ExecuteDrawingTool(DrawingTool.ArcCenterStartEnd);
        return true;
    }

    private bool TryExecuteEllipseModeCommand(string? command)
    {
        if (string.IsNullOrWhiteSpace(command))
        {
            return false;
        }

        var parts = command.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length < 2 || parts[0] is not ("ELLIPSE" or "EL"))
        {
            return false;
        }

        if (parts[1] is "C" or "CENTER")
        {
            if (parts.Length != 3 ||
                !double.TryParse(parts[2], NumberStyles.Float, CultureInfo.InvariantCulture, out var centerRatio) ||
                !double.IsFinite(centerRatio) || centerRatio <= 0 || centerRatio > 1)
            {
                UpdateStatus("Uso: ELLIPSE C <ratio 0..1>, por ejemplo ELLIPSE C 0.5");
                return true;
            }

            ExecuteDrawingTool(DrawingTool.EllipseCenter, centerRatio);
            return true;
        }

        if (parts[1] != "ARC")
        {
            return false;
        }

        if (parts.Length != 5 ||
            !double.TryParse(parts[2], NumberStyles.Float, CultureInfo.InvariantCulture, out var arcRatio) ||
            !double.TryParse(parts[3], NumberStyles.Float, CultureInfo.InvariantCulture, out var startDegrees) ||
            !double.TryParse(parts[4], NumberStyles.Float, CultureInfo.InvariantCulture, out var endDegrees) ||
            !double.IsFinite(arcRatio) || arcRatio <= 0 || arcRatio > 1 ||
            !double.IsFinite(startDegrees) || !double.IsFinite(endDegrees))
        {
            UpdateStatus("Uso: ELLIPSE ARC <ratio> <inicio°> <fin°>, por ejemplo ELLIPSE ARC 0.5 0 90");
            return true;
        }

        ExecuteDrawingTool(
            DrawingTool.EllipseArc,
            arcRatio,
            startDegrees * Math.PI / 180,
            endDegrees * Math.PI / 180);
        return true;
    }

    private void OnTitleBarPointerPressed(object? sender, PointerPressedEventArgs eventArgs)
    {
        if (!eventArgs.GetCurrentPoint(this).Properties.IsLeftButtonPressed ||
            IsInteractiveSource(eventArgs.Source))
        {
            return;
        }

        if (eventArgs.ClickCount == 2)
        {
            ToggleWindowState();
        }
        else
        {
            BeginMoveDrag(eventArgs);
        }

        eventArgs.Handled = true;
    }

    private void ToggleWindowState()
    {
        WindowState = WindowState == WindowState.Maximized
            ? WindowState.Normal
            : WindowState.Maximized;
        UpdateWindowChrome();
    }

    private void UpdateWindowChrome()
    {
        var maximized = WindowState == WindowState.Maximized;
        MaximizeButtonIcon.Tool = maximized ? "WindowRestore" : "WindowMaximize";
        AutomationProperties.SetName(
            MaximizeButton,
            maximized ? "Restaurar ventana" : "Maximizar ventana");
    }

    private ArcCadPoint ToWorld(Point point)
    {
        var world = WorkspaceViewport.ViewportToWorld(point);
        return new ArcCadPoint(world.X, world.Y);
    }

    private static float[] LineVertices(ArcCadPoint start, ArcCadPoint end) =>
        [(float)start.X, (float)start.Y, (float)end.X, (float)end.Y];

    private float[] PreviewVertices(ArcCadPoint start, ArcCadPoint end)
    {
        try
        {
            return _drawingTool switch
            {
                DrawingTool.Rectangle => Flatten(RectanglePreviewSegments(start, end)),
                DrawingTool.RevisionCloud => Flatten(RectangleSegments(start, end)),
                DrawingTool.Polygon => Flatten(PolygonSegments(
                    start,
                    end,
                    (int)_toolParameter,
                    _polygonCircumscribed)),
                DrawingTool.Circle => Flatten(CircleSegments(start, end)),
                DrawingTool.Multiline => Flatten(MultilineSegments(start, end)),
                DrawingTool.Stretch => Flatten(RectangleSegments(start, end)),
                DrawingTool.Move when _sourceLine is { } source =>
                    Flatten([Translate(source, end.X - start.X, end.Y - start.Y)]),
                DrawingTool.Copy when _sourceLine is { } source =>
                    Flatten([Translate(source, end.X - start.X, end.Y - start.Y)]),
                DrawingTool.Mirror when _sourceLine is { } source =>
                    Flatten([Segment(start, end), Mirror(source, start, end)]),
                _ => LineVertices(start, end),
            };
        }
        catch (ArgumentException)
        {
            return LineVertices(start, end);
        }
    }

    private float[] PathPreview(ArcCadPoint? cursor = null)
    {
        IEnumerable<ArcCadPoint> points = cursor is { } point
            ? _pathPoints.Append(point)
            : _pathPoints;
        return Flatten(points.Zip(points.Skip(1), Segment).ToArray());
    }

    private float[] WipeoutPreview(ArcCadPoint? cursor = null)
    {
        var points = (cursor is { } point ? _pathPoints.Append(point) : _pathPoints).ToArray();
        if (points.Length < 2)
        {
            return [];
        }

        return Flatten(
        [
            .. points.Zip(points.Skip(1), Segment),
            Segment(points[^1], points[0]),
        ]);
    }

    private void FinishPolyline(bool closed = false)
    {
        var points = _pathPoints.ToArray();
        CompleteNativeMutation(
            () => _session.CreatePolyline(points, closed),
            $"PLINE nativa {(closed ? "cerrada" : "abierta")} · {points.Length} vértices · 1 entidad");
    }

    private void FinishSpline()
    {
        var points = _pathPoints.ToArray();
        CompleteNativeMutation(
            () => _session.CreateSpline(points),
            $"SPLINE nativa · {points.Length} puntos de ajuste · 1 entidad");
    }

    private void FinishWipeout()
    {
        var points = _pathPoints.ToArray();
        CompleteNativeMutation(
            () => _session.CreateWipeout(points),
            $"WIPEOUT nativo · {points.Length} vértices · 1 máscara");
    }

    private void CreateContinuous(
        IReadOnlyList<(ArcCadPoint Start, ArcCadPoint End)> segments,
        ArcCadPoint end,
        string label)
    {
        CreateSegments(segments);
        _firstPoint = end;
        WorkspaceViewport.SetPreviewVertices([]);
        SetCursor(end);
        SyncScene();
        var status = $"{label} · {_session.Lines.Length} LINE · siguiente punto";
        _commandSession.MarkProgress(status);
        UpdateStatus(status);
    }

    private void CompleteTool(
        IReadOnlyList<(ArcCadPoint Start, ArcCadPoint End)> segments,
        string status)
    {
        CreateSegments(segments);
        ResetLineState();
        if (_commandSession.IsActive)
        {
            _commandSession.Complete(status);
        }
        SyncScene();
        UpdateStatus(status);
    }

    private void CompleteQuery(string result)
    {
        ResetLineState();
        UpdateControls();
        var status = CompactQueryResult(result);
        if (_commandSession.IsActive)
        {
            _commandSession.Complete(status);
        }
        UpdateStatus(status);
    }

    private void CompleteNativeMutation(Action mutation, string status)
    {
        mutation();
        _undoGroups.Push(1);
        _redoGroups.Clear();
        ResetLineState();
        if (_commandSession.IsActive)
        {
            _commandSession.Complete(status);
        }
        SyncScene();
        UpdateStatus(status);
    }

    private static string CompactQueryResult(string result) => string.Join(
        " · ",
        result.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries));

    private void CreateSegments(IReadOnlyList<(ArcCadPoint Start, ArcCadPoint End)> segments)
    {
        if (segments.Count == 0)
        {
            throw new ArgumentException("La operación debe crear al menos una LINE.", nameof(segments));
        }

        foreach (var segment in segments)
        {
            ValidateSegment(segment.Start, segment.End);
        }

        foreach (var segment in segments)
        {
            _session.CreateLine(segment.Start, segment.End);
        }

        _undoGroups.Push(segments.Count);
        _redoGroups.Clear();
    }

    private string RectangleModifierLabel()
    {
        var modifier = _rectangleFillet is { } radius
            ? $"FILLET {radius.ToString("G", CultureInfo.InvariantCulture)}"
            : _rectangleChamfer1 is { } first && _rectangleChamfer2 is { } second
                ? $"CHAMFER {first.ToString("G", CultureInfo.InvariantCulture)} {second.ToString("G", CultureInfo.InvariantCulture)}"
                : "BASIC";
        return _rectangleWidth is { } width
            ? $"{modifier} WIDTH {width.ToString("G", CultureInfo.InvariantCulture)}"
            : modifier;
    }

    private static void ValidateOptionalRectangleDistance(double? value, string parameterName)
    {
        if (value is { } number && (!double.IsFinite(number) || number <= 0.0))
        {
            throw new ArgumentOutOfRangeException(
                parameterName,
                number,
                "RECTANG requiere distancias finitas y positivas.");
        }
    }

    private (ArcCadPoint Start, ArcCadPoint End)[] RectanglePreviewSegments(
        ArcCadPoint first,
        ArcCadPoint opposite)
    {
        var minX = Math.Min(first.X, opposite.X);
        var maxX = Math.Max(first.X, opposite.X);
        var minY = Math.Min(first.Y, opposite.Y);
        var maxY = Math.Max(first.Y, opposite.Y);
        var width = maxX - minX;
        var height = maxY - minY;
        if (!double.IsFinite(width) || !double.IsFinite(height) ||
            width <= 0.000001 || height <= 0.000001)
        {
            throw new ArgumentException("RECTANG requiere ancho y alto mayores que cero.");
        }

        var bottomLeft = new ArcCadPoint(minX, minY);
        var bottomRight = new ArcCadPoint(maxX, minY);
        var topRight = new ArcCadPoint(maxX, maxY);
        var topLeft = new ArcCadPoint(minX, maxY);
        var minimumSide = Math.Min(width, height);
        if (_rectangleChamfer1 is { } incoming && _rectangleChamfer2 is { } outgoing)
        {
            if (!double.IsFinite(incoming + outgoing) || incoming + outgoing >= minimumSide)
            {
                throw new ArgumentException("RECTANG CHAMFER se solapa con el lado opuesto.");
            }

            ArcCadPoint[] points =
            [
                new ArcCadPoint(minX + outgoing, minY),
                new ArcCadPoint(maxX - incoming, minY),
                new ArcCadPoint(maxX, minY + outgoing),
                new ArcCadPoint(maxX, maxY - incoming),
                new ArcCadPoint(maxX - outgoing, maxY),
                new ArcCadPoint(minX + incoming, maxY),
                new ArcCadPoint(minX, maxY - outgoing),
                new ArcCadPoint(minX, minY + incoming),
            ];
            return ClosedPathSegments(points);
        }

        if (_rectangleFillet is { } radius)
        {
            if (radius >= minimumSide / 2.0)
            {
                throw new ArgumentException("RECTANG FILLET se solapa con el lado opuesto.");
            }

            var segments = new List<(ArcCadPoint Start, ArcCadPoint End)>(28)
            {
                Segment(
                    new ArcCadPoint(minX + radius, minY),
                    new ArcCadPoint(maxX - radius, minY)),
            };
            AddArcPreviewSegments(
                segments,
                new ArcCadPoint(maxX - radius, minY + radius),
                radius,
                -Math.PI / 2.0);
            segments.Add(Segment(
                new ArcCadPoint(maxX, minY + radius),
                new ArcCadPoint(maxX, maxY - radius)));
            AddArcPreviewSegments(
                segments,
                new ArcCadPoint(maxX - radius, maxY - radius),
                radius,
                0.0);
            segments.Add(Segment(
                new ArcCadPoint(maxX - radius, maxY),
                new ArcCadPoint(minX + radius, maxY)));
            AddArcPreviewSegments(
                segments,
                new ArcCadPoint(minX + radius, maxY - radius),
                radius,
                Math.PI / 2.0);
            segments.Add(Segment(
                new ArcCadPoint(minX, maxY - radius),
                new ArcCadPoint(minX, minY + radius)));
            AddArcPreviewSegments(
                segments,
                new ArcCadPoint(minX + radius, minY + radius),
                radius,
                Math.PI);
            return [.. segments];
        }

        return RectangleSegments(bottomLeft, topRight);
    }

    private static (ArcCadPoint Start, ArcCadPoint End)[] ClosedPathSegments(
        IReadOnlyList<ArcCadPoint> points)
    {
        var segments = new (ArcCadPoint Start, ArcCadPoint End)[points.Count];
        for (var index = 0; index < points.Count; index++)
        {
            segments[index] = Segment(points[index], points[(index + 1) % points.Count]);
        }

        return segments;
    }

    private static void AddArcPreviewSegments(
        ICollection<(ArcCadPoint Start, ArcCadPoint End)> segments,
        ArcCadPoint center,
        double radius,
        double startAngle)
    {
        const int SegmentCount = 6;
        for (var index = 0; index < SegmentCount; index++)
        {
            var firstAngle = startAngle + Math.PI / 2.0 * index / SegmentCount;
            var secondAngle = startAngle + Math.PI / 2.0 * (index + 1) / SegmentCount;
            segments.Add(Segment(
                PolarPoint(center, radius, firstAngle),
                PolarPoint(center, radius, secondAngle)));
        }
    }

    private static (ArcCadPoint Start, ArcCadPoint End)[] RectangleSegments(
        ArcCadPoint first,
        ArcCadPoint opposite)
    {
        var second = new ArcCadPoint(opposite.X, first.Y);
        var fourth = new ArcCadPoint(first.X, opposite.Y);
        return
        [
            Segment(first, second),
            Segment(second, opposite),
            Segment(opposite, fourth),
            Segment(fourth, first),
        ];
    }

    private static ArcCadPoint[] RevisionCloudContour(ArcCadPoint first, ArcCadPoint opposite)
    {
        var width = Math.Abs(opposite.X - first.X);
        var height = Math.Abs(opposite.Y - first.Y);
        if (!double.IsFinite(width) || !double.IsFinite(height) ||
            width <= 0.000001 || height <= 0.000001)
        {
            throw new ArgumentException("REVCLOUD requiere ancho y alto mayores que cero.");
        }

        return
        [
            first,
            new ArcCadPoint(opposite.X, first.Y),
            opposite,
            new ArcCadPoint(first.X, opposite.Y),
        ];
    }

    private static (ArcCadPoint Start, ArcCadPoint End)[] PolygonSegments(
        ArcCadPoint center,
        ArcCadPoint radiusPoint,
        int sides,
        bool circumscribed)
    {
        if (sides is < 3 or > 1024)
        {
            throw new ArgumentOutOfRangeException(nameof(sides));
        }

        var radius = Math.Sqrt(
            Math.Pow(radiusPoint.X - center.X, 2) +
            Math.Pow(radiusPoint.Y - center.Y, 2));
        if (!double.IsFinite(radius) || radius <= 0.000001)
        {
            throw new ArgumentException("El polígono necesita un radio mayor que cero.");
        }

        var circumradius = circumscribed ? radius / Math.Cos(Math.PI / sides) : radius;
        var segments = new (ArcCadPoint Start, ArcCadPoint End)[sides];
        for (var index = 0; index < sides; index++)
        {
            segments[index] = Segment(
                PolarPoint(center, circumradius, Math.Tau * index / sides),
                PolarPoint(center, circumradius, Math.Tau * (index + 1) / sides));
        }

        return segments;
    }

    private static (ArcCadPoint Start, ArcCadPoint End)[] MultilineSegments(
        ArcCadPoint start,
        ArcCadPoint end)
    {
        var dx = end.X - start.X;
        var dy = end.Y - start.Y;
        var length = Math.Sqrt(dx * dx + dy * dy);
        if (!double.IsFinite(length) || length <= 0.000001)
        {
            throw new ArgumentException("El muro necesita dos puntos distintos.");
        }

        var offsetX = -dy / length * WallHalfWidth;
        var offsetY = dx / length * WallHalfWidth;
        return
        [
            Segment(
                new ArcCadPoint(start.X + offsetX, start.Y + offsetY),
                new ArcCadPoint(end.X + offsetX, end.Y + offsetY)),
            Segment(
                new ArcCadPoint(start.X - offsetX, start.Y - offsetY),
                new ArcCadPoint(end.X - offsetX, end.Y - offsetY)),
        ];
    }

    private static (ArcCadPoint Start, ArcCadPoint End)[] CircleSegments(
        ArcCadPoint center,
        ArcCadPoint radiusPoint)
    {
        var radius = Math.Sqrt(
            Math.Pow(radiusPoint.X - center.X, 2) +
            Math.Pow(radiusPoint.Y - center.Y, 2));
        if (!double.IsFinite(radius) || radius <= 0.000001)
        {
            throw new ArgumentException("El círculo necesita un radio mayor que cero.");
        }

        var segments = new (ArcCadPoint Start, ArcCadPoint End)[CircleSegmentCount];
        for (var index = 0; index < segments.Length; index++)
        {
            var firstAngle = Math.Tau * index / segments.Length;
            var secondAngle = Math.Tau * (index + 1) / segments.Length;
            segments[index] = Segment(
                PolarPoint(center, radius, firstAngle),
                PolarPoint(center, radius, secondAngle));
        }

        return segments;
    }

    private static (ArcCadPoint Start, ArcCadPoint End)[] EllipseSegments(
        ArcCadPoint center,
        ArcCadPoint axisEnd,
        ArcCadPoint minorPoint)
    {
        var ratio = EllipseRatio(center, axisEnd, minorPoint);
        var dx = axisEnd.X - center.X;
        var dy = axisEnd.Y - center.Y;
        var major = Math.Sqrt(dx * dx + dy * dy);
        var ux = dx / major;
        var uy = dy / major;
        var minor = major * ratio;
        ArcCadPoint At(double angle) => new(
            center.X + major * Math.Cos(angle) * ux - minor * Math.Sin(angle) * uy,
            center.Y + major * Math.Cos(angle) * uy + minor * Math.Sin(angle) * ux);

        var segments = new (ArcCadPoint Start, ArcCadPoint End)[CircleSegmentCount];
        for (var index = 0; index < segments.Length; index++)
        {
            segments[index] = Segment(
                At(Math.Tau * index / segments.Length),
                At(Math.Tau * (index + 1) / segments.Length));
        }

        return segments;
    }

    private static double EllipseRatio(
        ArcCadPoint center,
        ArcCadPoint axisEnd,
        ArcCadPoint minorPoint)
    {
        ValidateSegment(center, axisEnd);
        ValidateSegment(center, minorPoint);
        var dx = axisEnd.X - center.X;
        var dy = axisEnd.Y - center.Y;
        var major = Math.Sqrt(dx * dx + dy * dy);
        var minor = Math.Abs(
            dx * (minorPoint.Y - center.Y) - dy * (minorPoint.X - center.X)) / major;
        if (!double.IsFinite(minor) || minor <= 0.000001 || minor > major + 0.000001)
        {
            throw new ArgumentException("El semieje menor debe ser perpendicular, positivo y no mayor que el principal.");
        }

        return Math.Min(1, minor / major);
    }

    private static (ArcCadPoint Start, ArcCadPoint End)[] ArcSegments(
        ArcCadPoint start,
        ArcCadPoint middle,
        ArcCadPoint end)
    {
        var determinant = 2 * (
            start.X * (middle.Y - end.Y) +
            middle.X * (end.Y - start.Y) +
            end.X * (start.Y - middle.Y));
        if (!double.IsFinite(determinant) || Math.Abs(determinant) <= 0.000001)
        {
            throw new ArgumentException("Los tres puntos del arco no pueden ser colineales.");
        }

        var startSquared = start.X * start.X + start.Y * start.Y;
        var middleSquared = middle.X * middle.X + middle.Y * middle.Y;
        var endSquared = end.X * end.X + end.Y * end.Y;
        var center = new ArcCadPoint(
            (startSquared * (middle.Y - end.Y) +
                middleSquared * (end.Y - start.Y) +
                endSquared * (start.Y - middle.Y)) / determinant,
            (startSquared * (end.X - middle.X) +
                middleSquared * (start.X - end.X) +
                endSquared * (middle.X - start.X)) / determinant);
        var radius = Math.Sqrt(Math.Pow(start.X - center.X, 2) + Math.Pow(start.Y - center.Y, 2));
        var startAngle = Math.Atan2(start.Y - center.Y, start.X - center.X);
        var middleAngle = Math.Atan2(middle.Y - center.Y, middle.X - center.X);
        var endAngle = Math.Atan2(end.Y - center.Y, end.X - center.X);
        var middleSweep = NormalizeAngle(middleAngle - startAngle);
        var endSweep = NormalizeAngle(endAngle - startAngle);
        var sweep = middleSweep <= endSweep ? endSweep : endSweep - Math.Tau;
        var segmentCount = Math.Clamp((int)Math.Ceiling(Math.Abs(sweep) / (Math.PI / 18)), 6, 32);
        var segments = new (ArcCadPoint Start, ArcCadPoint End)[segmentCount];
        for (var index = 0; index < segments.Length; index++)
        {
            segments[index] = Segment(
                PolarPoint(center, radius, startAngle + sweep * index / segments.Length),
                PolarPoint(center, radius, startAngle + sweep * (index + 1) / segments.Length));
        }

        return segments;
    }

    private static (ArcCadPoint Start, ArcCadPoint End) Mirror(
        CadLine source,
        ArcCadPoint axisStart,
        ArcCadPoint axisEnd) =>
        Segment(
            Reflect(new ArcCadPoint(source.X1, source.Y1), axisStart, axisEnd),
            Reflect(new ArcCadPoint(source.X2, source.Y2), axisStart, axisEnd));

    private static ArcCadPoint Reflect(ArcCadPoint point, ArcCadPoint axisStart, ArcCadPoint axisEnd)
    {
        var dx = axisEnd.X - axisStart.X;
        var dy = axisEnd.Y - axisStart.Y;
        var lengthSquared = dx * dx + dy * dy;
        if (!double.IsFinite(lengthSquared) || lengthSquared <= 0.000001)
        {
            throw new ArgumentException("El eje de simetría necesita dos puntos distintos.");
        }

        var projection = ((point.X - axisStart.X) * dx + (point.Y - axisStart.Y) * dy) / lengthSquared;
        var projectedX = axisStart.X + projection * dx;
        var projectedY = axisStart.Y + projection * dy;
        return new ArcCadPoint(2 * projectedX - point.X, 2 * projectedY - point.Y);
    }

    private static (ArcCadPoint Start, ArcCadPoint End) Translate(CadLine source, double x, double y) =>
        Segment(
            new ArcCadPoint(source.X1 + x, source.Y1 + y),
            new ArcCadPoint(source.X2 + x, source.Y2 + y));

    private static (ArcCadPoint Start, ArcCadPoint End) Segment(ArcCadPoint start, ArcCadPoint end) =>
        (start, end);

    private static ArcCadPoint PolarPoint(ArcCadPoint center, double radius, double angle) =>
        new(center.X + Math.Cos(angle) * radius, center.Y + Math.Sin(angle) * radius);

    private static double NormalizeAngle(double angle)
    {
        var normalized = angle % Math.Tau;
        return normalized < 0 ? normalized + Math.Tau : normalized;
    }

    private static float[] Flatten(IReadOnlyList<(ArcCadPoint Start, ArcCadPoint End)> segments)
    {
        var vertices = new float[segments.Count * 4];
        for (var index = 0; index < segments.Count; index++)
        {
            vertices[index * 4] = (float)segments[index].Start.X;
            vertices[index * 4 + 1] = (float)segments[index].Start.Y;
            vertices[index * 4 + 2] = (float)segments[index].End.X;
            vertices[index * 4 + 3] = (float)segments[index].End.Y;
        }

        return vertices;
    }

    private static void ValidateSegment(ArcCadPoint start, ArcCadPoint end)
    {
        if (!double.IsFinite(start.X) || !double.IsFinite(start.Y) ||
            !double.IsFinite(end.X) || !double.IsFinite(end.Y) ||
            Math.Abs(start.X - end.X) + Math.Abs(start.Y - end.Y) <= 0.000001)
        {
            throw new ArgumentException("Cada LINE necesita dos puntos finitos y distintos.");
        }
    }

    private static double RotationAngle(
        ArcCadPoint basePoint,
        ArcCadPoint referencePoint,
        ArcCadPoint targetPoint)
    {
        ValidateSegment(basePoint, referencePoint);
        ValidateSegment(basePoint, targetPoint);
        var angle = Math.Atan2(targetPoint.Y - basePoint.Y, targetPoint.X - basePoint.X) -
            Math.Atan2(referencePoint.Y - basePoint.Y, referencePoint.X - basePoint.X);
        if (!double.IsFinite(angle) || Math.Abs(angle) <= 0.000000001)
        {
            throw new ArgumentException("El giro necesita dos direcciones distintas.");
        }

        return angle;
    }

    private CadLine RequireSelectedLine(string action) =>
        _session.SelectedLine ?? throw new InvalidOperationException($"Seleccione una LINE antes de {action}.");

    private CadLine SelectOtherLine(ArcCadPoint point)
    {
        var source = _sourceLine ?? throw new InvalidOperationException("No hay LINE de origen.");
        _session.SelectAt(point, HitTolerance / WorkspaceViewport.Zoom);
        SyncSelection();
        UpdateControls();
        var selected = _session.SelectedLine ??
            throw new InvalidOperationException("Seleccione una segunda LINE.");
        if (selected.EntityId == source.EntityId)
        {
            throw new InvalidOperationException("Seleccione una LINE distinta de la primera.");
        }

        return selected;
    }

    private static string ToolCommandName(DrawingTool tool) => tool switch
    {
        DrawingTool.Rectangle => "RECTANG",
        DrawingTool.Polyline => "PLINE",
        DrawingTool.CircleTwoPoint => "CIRCLE 2P",
        DrawingTool.CircleThreePoint => "CIRCLE 3P",
        DrawingTool.CircleTtr => "CIRCLE TTR",
        DrawingTool.ArcCenterStartEnd => "ARC CSE",
        DrawingTool.EllipseCenter => "ELLIPSE C",
        DrawingTool.EllipseArc => "ELLIPSE ARC",
        DrawingTool.XlineHorizontal => "XLINE H",
        DrawingTool.XlineVertical => "XLINE V",
        DrawingTool.XlineAngle => "XLINE A",
        DrawingTool.PointId => "ID",
        _ => tool.ToString().ToUpperInvariant(),
    };

    private static string ToolLabel(DrawingTool tool) => tool switch
    {
        DrawingTool.Line => "LINE",
        DrawingTool.Polyline => "Polilínea",
        DrawingTool.Rectangle => "Rectángulo",
        DrawingTool.Polygon => "Polígono",
        DrawingTool.Circle => "Círculo",
        DrawingTool.CircleTwoPoint => "Círculo 2P",
        DrawingTool.CircleThreePoint => "Círculo 3P",
        DrawingTool.CircleTtr => "Círculo TTR",
        DrawingTool.Arc => "Arco",
        DrawingTool.ArcCenterStartEnd => "Arco centro-inicio-fin",
        DrawingTool.Ellipse => "Elipse",
        DrawingTool.EllipseCenter => "Elipse por centro",
        DrawingTool.EllipseArc => "Arco elíptico",
        DrawingTool.Spline => "Spline",
        DrawingTool.RevisionCloud => "Nube de revisión",
        DrawingTool.Wipeout => "Wipeout",
        DrawingTool.Point => "Point",
        DrawingTool.Xline => "Xline",
        DrawingTool.XlineHorizontal => "Xline horizontal",
        DrawingTool.XlineVertical => "Xline vertical",
        DrawingTool.XlineAngle => "Xline angular",
        DrawingTool.Ray => "Ray",
        DrawingTool.Donut => "Donut",
        DrawingTool.Multiline => "Muro doble",
        DrawingTool.Move => "Desplazar",
        DrawingTool.Rotate => "Girar",
        DrawingTool.Scale => "Escala",
        DrawingTool.Offset => "Desfase",
        DrawingTool.Trim => "Recortar",
        DrawingTool.Extend => "Extender",
        DrawingTool.Chamfer => "Chaflán",
        DrawingTool.Fillet => "Empalme",
        DrawingTool.Break => "Partir",
        DrawingTool.BreakAtPoint => "Dividir en punto",
        DrawingTool.Lengthen => "Cambiar longitud",
        DrawingTool.Stretch => "Estirar",
        DrawingTool.Join => "Unir",
        DrawingTool.Align => "Alinear",
        DrawingTool.Copy => "Copiar",
        DrawingTool.Mirror => "Simetría",
        DrawingTool.Distance => "Distancia",
        DrawingTool.PointId => "ID de punto",
        DrawingTool.Angle => "Ángulo",
        _ => "Dibujo",
    };

    private void SetCursor(ArcCadPoint point) =>
        WorkspaceViewport.SetCursor((float)point.X, (float)point.Y, _lastSnap is not null);

    private void SyncScene()
    {
        WorkspaceViewport.SetScene(_session.Entities.Span, _session.Markers.Span);
        SyncSelection();
        RefreshLayerPanel();
        UpdateControls();
    }

    private void ResetLineState()
    {
        _lineState = LineState.Idle;
        _drawingTool = DrawingTool.None;
        _firstPoint = null;
        _secondPoint = null;
        _thirdPoint = null;
        _pathPoints.Clear();
        _sourceLine = null;
        _toolParameter = 0;
        _secondaryToolParameter = 0;
        _rectangleChamfer1 = null;
        _rectangleChamfer2 = null;
        _rectangleFillet = null;
        _rectangleWidth = null;
        _polygonCircumscribed = false;
        _lastSnap = null;
        WorkspaceViewport.SetPreviewVertices([]);
        WorkspaceViewport.ClearCursor();
    }

    private void ResetHistoryGroups()
    {
        _undoGroups.Clear();
        _redoGroups.Clear();
    }

    private void SetPanMode(bool active)
    {
        _panMode = active;
        if (!active)
        {
            _isPanning = false;
        }
    }

    private void SyncSelection()
    {
        WorkspaceViewport.SetSelectedEntity(_session.SelectedEntityId);
        if (_session.SelectedEntityId is not { } entityId)
        {
            PropertiesPanel.IsVisible = false;
            PropertiesSelectionCombo.SelectedIndex = 0;
            PropertyIdText.Text = "—";
            PropertyLengthText.Text = "—";
            return;
        }

        PropertiesPanel.IsVisible = true;
        PropertiesSelectionCombo.SelectedIndex = 1;
        var entityType = _session.SelectedEntityType ?? "ENTIDAD";
        PropertyTypeText.Text = entityType;
        PropertyIdText.Text = entityId.ToString(CultureInfo.InvariantCulture);
        string detail;
        string accessibleDetail;
        if (_session.SelectedEntity is { } entity)
        {
            detail = entity.Length.ToString("F2", CultureInfo.InvariantCulture);
            var lengthLabel = entity.AnalyticLength.HasValue || entityType == "LINE"
                ? "longitud"
                : "longitud visible";
            accessibleDetail = $"{lengthLabel} {detail}";
        }
        else if (_session.SelectedMarker is { } marker)
        {
            detail = $"X {marker.X:F2} · Y {marker.Y:F2}";
            accessibleDetail = $"coordenadas {detail}";
        }
        else
        {
            throw new InvalidOperationException("La selección nativa no existe en la escena visible.");
        }

        PropertyLengthText.Text = detail;
        AutomationProperties.SetName(
            PropertiesPanel,
            $"Propiedades {entityType}, ID {entityId}, {accessibleDetail}");
    }

    private void UpdateControls()
    {
        var toolAvailable = !IsLineActive;
        var lineSelectionAvailable = toolAvailable && _session.SelectedLine is not null;
        var entitySelectionAvailable = toolAvailable && _session.SelectedEntityId is not null;
        foreach (var button in new[]
        {
            LineButton,
            RailLineButton,
            HomePolylineButton,
            RailPolylineButton,
            HomeRectangleButton,
            HomeCircleButton,
            RailCircleButton,
            HomeArcButton,
            RailArcButton,
            HomeMultilineButton,
        })
        {
            button.IsEnabled = toolAvailable;
        }

        foreach (var button in new[]
        {
            HomeMoveButton,
            HomeRotateButton,
            HomeCopyButton,
            RailCopyButton,
            HomeMirrorButton,
            RailMirrorButton,
            HomeArrayButton,
        })
        {
            button.IsEnabled = lineSelectionAvailable;
        }

        SetActive(LineButton, _drawingTool == DrawingTool.Line);
        SetActive(RailLineButton, _drawingTool == DrawingTool.Line);
        SetActive(HomePolylineButton, _drawingTool == DrawingTool.Polyline);
        SetActive(RailPolylineButton, _drawingTool == DrawingTool.Polyline);
        SetActive(HomeRectangleButton, _drawingTool == DrawingTool.Rectangle);
        SetActive(HomeCircleButton, _drawingTool == DrawingTool.Circle);
        SetActive(RailCircleButton, _drawingTool == DrawingTool.Circle);
        SetActive(HomeArcButton, _drawingTool == DrawingTool.Arc);
        SetActive(RailArcButton, _drawingTool == DrawingTool.Arc);
        SetActive(HomeMultilineButton, _drawingTool == DrawingTool.Multiline);
        SetActive(HomeMoveButton, _drawingTool == DrawingTool.Move);
        SetActive(HomeRotateButton, _drawingTool == DrawingTool.Rotate);
        SetActive(HomeCopyButton, _drawingTool == DrawingTool.Copy);
        SetActive(RailCopyButton, _drawingTool == DrawingTool.Copy);
        SetActive(HomeMirrorButton, _drawingTool == DrawingTool.Mirror);
        SetActive(RailMirrorButton, _drawingTool == DrawingTool.Mirror);
        foreach (var button in RibbonContextPanel.GetVisualDescendants().OfType<Button>())
        {
            switch (button.Tag?.ToString())
            {
                case "ribbon.herramientas.distancia":
                    button.IsEnabled = toolAvailable;
                    SetActive(button, _drawingTool == DrawingTool.Distance);
                    break;
                case "ribbon.herramientas.angulo":
                    button.IsEnabled = toolAvailable;
                    SetActive(button, _drawingTool == DrawingTool.Angle);
                    break;
                case "ribbon.herramientas.id-de-punto":
                    button.IsEnabled = toolAvailable;
                    SetActive(button, _drawingTool == DrawingTool.PointId);
                    break;
                case "ribbon.herramientas.area":
                    button.IsEnabled = entitySelectionAvailable;
                    break;
                case "ribbon.herramientas.lista":
                    button.IsEnabled = entitySelectionAvailable;
                    break;
            }
        }
        SetActive(SelectToolButton, !IsLineActive && !_panMode);
        SetActive(PanButton, _panMode);
        SetActive(GridStatusButton, WorkspaceViewport.ShowGrid);
        SetActive(UcsButton, WorkspaceViewport.ShowUcs);
        SetActive(OsnapStatusButton, _objectSnapEnabled);
        SetActive(OrthoStatusButton, _orthoEnabled);
        SetActive(HomeDimensionButton, WorkspaceViewport.ShowDimensions);
        SetActive(RailDimensionButton, WorkspaceViewport.ShowDimensions);
        SetActive(AnnotationStatusButton, WorkspaceViewport.ShowDimensions);
        SetActive(LineweightStatusButton, WorkspaceViewport.UseHeavyLineweight);

        NewButton.IsEnabled = !_session.IsDirty;
        OpenButton.IsEnabled = !_session.IsDirty;
        NewDocumentButton.IsEnabled = !_session.IsDirty;
        NewWorkspaceDocumentButton.IsEnabled = !_session.IsDirty;
        UndoButton.IsEnabled = CanUndo;
        RedoButton.IsEnabled = CanRedo;

        var fileName = _session.CurrentPath is null
            ? "Sin título.arcf"
            : Path.GetFileName(_session.CurrentPath);
        var displayName = _session.IsDirty ? $"{fileName}*" : fileName;
        DocumentTitleText.Text = displayName;
        DocumentTabButton.Content = displayName;
        Title = $"ArcCAD Alpha — {displayName}";
        AutomationProperties.SetName(DocumentTabButton, $"Seleccionar {displayName}");
        AutomationProperties.SetName(CloseDocumentButton, $"Cerrar {displayName}");
    }

    private void UpdateStatus(string status)
    {
        _commandSession.SetPrompt(status);
        BackendStatusText.Text = status;
        AutomationProperties.SetName(BackendStatusText, status);
    }

    private void UpdateUnavailable(Control source)
    {
        var capability = AutomationProperties.GetName(source);
        UpdateUnavailable(string.IsNullOrWhiteSpace(capability)
            ? source.Tag?.ToString() ?? "capacidad"
            : capability);
    }

    private void UpdateUnavailable(string capability) =>
        UpdateStatus($"No disponible: {capability} aún no está conectada");

    private void SetPropertiesDockVisible(bool visible)
    {
        PropertiesDock.IsVisible = visible;
        PropertiesSplitter.IsVisible = visible;
        WorkspaceGrid.ColumnDefinitions[3].Width = visible ? new GridLength(6) : new GridLength(0);
        WorkspaceGrid.ColumnDefinitions[4].Width = visible ? new GridLength(347) : new GridLength(0);
    }

    private static void TogglePanel(Control panel, Control source)
    {
        panel.IsVisible = !panel.IsVisible;
        SetActive(source, panel.IsVisible);
    }

    private static void SetActive(Control control, bool active)
    {
        if (active)
        {
            if (!control.Classes.Contains("active"))
            {
                control.Classes.Add("active");
            }
        }
        else
        {
            control.Classes.Remove("active");
        }
    }

    private void DisableUnsupportedEditors()
    {
        foreach (var comboBox in this.GetVisualDescendants().OfType<ComboBox>())
        {
            if (!ReferenceEquals(comboBox, PropertiesSelectionCombo) &&
                !ReferenceEquals(comboBox, CurrentLayerCombo))
            {
                comboBox.IsEnabled = false;
            }
        }

        foreach (var textBox in this.GetVisualDescendants().OfType<TextBox>())
        {
            if (!ReferenceEquals(textBox, CommandInput) &&
                !ReferenceEquals(textBox, LayerSearchBox))
            {
                textBox.IsReadOnly = true;
            }
        }
    }

    private static bool IsInteractiveSource(object? source)
    {
        if (source is not Visual visual)
        {
            return false;
        }

        return visual is Button or TextBox ||
            visual.GetVisualAncestors().Any(ancestor => ancestor is Button or TextBox);
    }

    private void OnClosing(object? sender, WindowClosingEventArgs eventArgs)
    {
        if (!_session.IsDirty)
        {
            return;
        }

        eventArgs.Cancel = true;
        UpdateStatus(SaveBeforeCloseStatus);
    }

    private void OnClosed(object? sender, EventArgs eventArgs) => _session.Dispose();

    private enum LineState
    {
        Idle,
        AwaitingFirst,
        AwaitingNext,
        AwaitingThird,
        AwaitingFourth,
    }

    private enum DrawingTool
    {
        None,
        Line,
        Polyline,
        Rectangle,
        Polygon,
        Circle,
        CircleTwoPoint,
        CircleThreePoint,
        CircleTtr,
        Arc,
        ArcCenterStartEnd,
        Ellipse,
        EllipseCenter,
        EllipseArc,
        Spline,
        RevisionCloud,
        Wipeout,
        Point,
        Xline,
        XlineHorizontal,
        XlineVertical,
        XlineAngle,
        Ray,
        Donut,
        Multiline,
        Move,
        Rotate,
        Scale,
        Offset,
        Trim,
        Extend,
        Chamfer,
        Fillet,
        Break,
        BreakAtPoint,
        Lengthen,
        Stretch,
        Join,
        Align,
        Copy,
        Mirror,
        Distance,
        PointId,
        Angle,
    }
}
