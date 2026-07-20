using System.Diagnostics;
using System.Globalization;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Input;
using Avalonia.Input.Raw;
using Avalonia.Interactivity;
using Avalonia.LogicalTree;
using Avalonia.Media;
using Avalonia.Threading;
using Avalonia.VisualTree;
using ArcForge.Desktop;
using ArcForge.Desktop.Frontend;
using ArcForge.Native;

namespace ArcForge.Desktop.Screenshot;

internal static class Program
{
    private const string ConnectedStatus = "Motor conectado · LINE listo";
    private static readonly Color Amber = Color.FromArgb(255, 0xFB, 0xBF, 0x24);
    private static readonly Color Cyan = Color.FromArgb(255, 0x22, 0xD3, 0xEE);
    private static readonly Color Drawing = Color.FromArgb(255, 0xD6, 0xDC, 0xE2);
    private static int _checkCount;

    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
            Run(args);
            Console.WriteLine($"headless-check: PASS checks={_checkCount}");
            return 0;
        }
        catch (Exception error)
        {
            Console.Error.WriteLine($"headless-check: {error}");
            return 1;
        }
    }

    private static void Run(string[] args)
    {
        Environment.SetEnvironmentVariable(
            "ARCFORGE_PGP_PATH",
            Path.Combine(Path.GetTempPath(), $"ArcCAD-Alias-Isolated-{Guid.NewGuid():N}", "Aliases.pgp"));
        Check(args.Length is 3 or 4, "usage: <af_ffi.dll> <sha256> <libunwind.dll> [capture.png]");

        var expectedDllPath = Path.GetFullPath(args[0]);
        var expectedDllHash = args[1].Trim().ToLowerInvariant();
        var expectedUnwindPath = Path.GetFullPath(args[2]);
        var capturePath = args.Length == 4 ? Path.GetFullPath(args[3]) : null;
        Check(File.Exists(expectedDllPath), "af_ffi.dll path");
        Check(File.Exists(expectedUnwindPath), "libunwind.dll path");
        Check(
            string.Equals(Path.GetFileName(expectedDllPath), "af_ffi.dll", StringComparison.OrdinalIgnoreCase),
            "af_ffi.dll filename");
        Check(
            string.Equals(Path.GetFileName(expectedUnwindPath), "libunwind.dll", StringComparison.OrdinalIgnoreCase),
            "libunwind.dll filename");
        Check(expectedDllHash.Length == 64 && expectedDllHash.All(Uri.IsHexDigit), "af_ffi.dll sha256 argument");
        Check(
            string.Equals(FileHash(expectedDllPath), expectedDllHash, StringComparison.OrdinalIgnoreCase),
            "af_ffi.dll sha256 input");

        var applicationDirectory = Path.TrimEndingDirectorySeparator(Path.GetFullPath(AppContext.BaseDirectory));
        Check(PathsEqual(Path.GetDirectoryName(expectedDllPath)!, applicationDirectory), "af_ffi.dll must be app-local");
        Check(PathsEqual(Path.GetDirectoryName(expectedUnwindPath)!, applicationDirectory), "libunwind.dll must be app-local");
        if (capturePath is not null)
        {
            Check(
                string.Equals(Path.GetExtension(capturePath), ".png", StringComparison.OrdinalIgnoreCase),
                "capture filename must end in .png");
            Check(Directory.Exists(Path.GetDirectoryName(capturePath)), "capture directory");
        }

        AppBuilder.Configure<Application>()
            .UseSkia()
            .UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false })
            .SetupWithoutStarting();

        var capture = RunHeadless(capturePath);
        RunCommandSessionContract();
        RunCommandSessionUi();
        RunLayerLifecycle();
        RunEraseLastLine();
        RunNativeEditCommands();
        RunNativeEditCommandsII();
        RunAlignmentCleanupCommands();
        RunClosedShapeCommands();
        RunRectangleModifierCommands(capturePath is null ? null : Path.GetDirectoryName(capturePath));
        RunPgpAliases(capturePath is null ? null : Path.GetDirectoryName(capturePath));
        RunPostCommitFault();

        using var process = Process.GetCurrentProcess();
        var loadedDllPath = FindModulePath(process, "af_ffi.dll");
        var loadedUnwindPath = FindModulePath(process, "libunwind.dll");
        Check(PathsEqual(loadedDllPath, expectedDllPath), "loaded af_ffi.dll path");
        Check(PathsEqual(loadedUnwindPath, expectedUnwindPath), "loaded libunwind.dll path");
        Check(
            string.Equals(FileHash(loadedDllPath), expectedDllHash, StringComparison.OrdinalIgnoreCase),
            "loaded af_ffi.dll sha256");
        if (capturePath is not null)
        {
            File.WriteAllBytes(capturePath, capture);
        }
    }

    private static byte[] RunHeadless(string? capturePath)
    {
        var tempDirectory = Path.Combine(Path.GetTempPath(), $"ArcCAD-Desktop-Harness-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDirectory);
        var documentPath = Path.Combine(tempDirectory, "rectangle.arcf");
        MainWindow? window = null;
        byte[]? capture = null;
        try
        {
            window = new MainWindow
            {
                WindowState = WindowState.Normal,
                Width = 1672,
                Height = 941,
            };
            window.Show();
            Dispatcher.UIThread.RunJobs();

            Check(window.ClientSize == new Size(1672, 941), "1672x941 client size");
            Check(window.MinWidth == 1280 && window.MinHeight == 720, "minimum window size");
            Check(window.Title == "ArcCAD Alpha — Sin título.arcf", "window title");
            Check(window.SystemDecorations == SystemDecorations.None, "custom window chrome");
            var tree = ReadTree(window);
            CheckLayout(window, tree, new Size(1672, 941));
            CheckSingleBackend(window, tree);

            window.Width = 1280;
            window.Height = 720;
            Dispatcher.UIThread.RunJobs();
            CheckLayout(window, tree, new Size(1280, 720));
            var compactFrame = CaptureFrame(window);
            Check(compactFrame.PixelSize == new PixelSize(1280, 720), "1280x720 rendered frame");
            if (capturePath is not null)
            {
                File.WriteAllBytes(
                    Path.Combine(
                        Path.GetDirectoryName(capturePath)!,
                        $"{Path.GetFileNameWithoutExtension(capturePath)}-1280x720.png"),
                    compactFrame.Png);
            }
            window.Width = 1672;
            window.Height = 941;
            Dispatcher.UIThread.RunJobs();
            CheckLayout(window, tree, new Size(1672, 941));

            Check(window.FocusManager is not null, "focus manager");
            window.FocusManager!.ClearFocus();
            Dispatcher.UIThread.RunJobs();
            Check(!tree.NewButton.IsFocused && !tree.OpenButton.IsFocused &&
                !tree.SaveButton.IsFocused && !tree.LineButton.IsFocused &&
                !tree.UndoButton.IsFocused && !tree.RedoButton.IsFocused &&
                !tree.Viewport.IsFocused, "initial focus");

            Check(tree.Viewport.IsVisible && tree.Viewport.Focusable, "viewport visibility/focusability");
            Check(tree.Viewport.Child is null, "viewport children");
            Check(tree.Viewport.ShowGrid && tree.Viewport.ShowUcs, "viewport overlays default");
            Check(AutomationProperties.GetName(tree.Viewport) == "Área de dibujo", "viewport accessible name");
            CheckActionButton(tree.NewButton, "qat.new", "Nuevo documento", enabled: true);
            CheckActionButton(tree.OpenButton, "qat.open", "Abrir documento .arcf", enabled: true);
            CheckActionButton(tree.SaveButton, "qat.save", "Guardar documento .arcf", enabled: true);
            CheckActionButton(tree.LineButton, "home.draw.line", "Dibujar LINE", enabled: true);
            CheckActionButton(tree.RailLineButton, "workspace.tool.line", "Línea", enabled: true);
            CheckActionButton(tree.PolylineButton, "home.draw.polyline", "Polilínea", enabled: true);
            CheckActionButton(tree.RectangleButton, "home.draw.rectangle", "Rectángulo", enabled: true);
            CheckActionButton(tree.CircleButton, "home.draw.circle", "Círculo", enabled: true);
            CheckActionButton(tree.ArcButton, "home.draw.arc", "Arco", enabled: true);
            CheckActionButton(tree.MultilineButton, "home.annotate.multiline", "Multilínea", enabled: true);
            CheckActionButton(tree.MoveButton, "home.modify.move", "Desplazar", enabled: false);
            CheckActionButton(tree.RotateButton, "home.modify.rotate", "Girar", enabled: false);
            CheckActionButton(tree.CopyButton, "home.modify.copy", "Copiar", enabled: false);
            CheckActionButton(tree.MirrorButton, "home.modify.mirror", "Simetría", enabled: false);
            CheckActionButton(tree.ArrayButton, "home.draw.array", "Matriz", enabled: false);
            CheckActionButton(tree.DimensionButton, "home.annotate.dimension", "Cota", enabled: true);
            CheckActionButton(tree.ZoomInButton, "view.nav.zoom", "Acercar", enabled: true);
            CheckActionButton(tree.PanButton, "view.nav.pan", "Encuadre", enabled: true);
            CheckActionButton(tree.FitViewButton, "view.nav.view", "Ajustar dibujo", enabled: true);
            CheckActionButton(tree.ResetViewButton, "view.nav.more", "Restablecer vista", enabled: true);
            CheckActionButton(tree.UndoButton, "qat.undo", "Deshacer última acción", enabled: false);
            CheckActionButton(tree.RedoButton, "qat.redo", "Rehacer última acción", enabled: false);
            CheckActionButton(tree.NewDocumentButton, "shell.new-document", "Nuevo dibujo", enabled: true);
            CheckActionButton(tree.NewWorkspaceDocumentButton, "document.new", "Nuevo dibujo", enabled: true);
            CheckActionButton(tree.HomeRibbonTab, "ribbon.tab.inicio", "Pestaña Inicio", enabled: true);
            CheckActionButton(tree.ToolsRibbonTab, "ribbon.tab.herramientas", "Pestaña Herramientas", enabled: true);
            Check(tree.Ribbon.IsVisible && !tree.RibbonContext.IsVisible, "home ribbon initially visible");
            Check(tree.PropertiesDock.IsVisible, "properties dock initially visible");
            Check(!tree.Properties.IsVisible, "properties initially hidden");
            Check(!tree.PropertiesSelection.IsEnabled && tree.PropertiesSelection.SelectedIndex == 0,
                "properties selection read-only");
            Check(tree.UnsupportedProperty.Text == "No disponible", "unsupported property honesty");
            Check(!tree.CommandInput.IsReadOnly, "productive command input");

            var controls = window.GetLogicalDescendants().OfType<Control>().Prepend(window).ToArray();
            Check(controls.Count(control => control is CadViewport) == 1, "viewport count");
            Check(controls.OfType<Button>().Count() >= 80, "existing UI button population");
            Check(
                window.GetVisualDescendants().OfType<Control>().Prepend(window)
                    .All(control => control.Transitions is null || control.Transitions.Count == 0),
                "all UI transitions disabled");
            tree.Viewport.Focus();
            Dispatcher.UIThread.RunJobs();
            Check(tree.Viewport.IsFocused, "viewport focus");

            Check(tree.StatusText.Text == ConnectedStatus, "initial status text");
            Check(AutomationProperties.GetName(tree.StatusText) == ConnectedStatus, "initial status accessible name");
            Check(tree.DocumentTitle.Text == "Sin título.arcf", "real document title");
            Check(window.IsBackendConnected, "backend connected state");
            Check(window.Lines.Length == 0 && window.LineEntityId == 0, "empty session geometry");
            Check(window.LastTransactionSequence is null, "empty session transaction sequence");
            Check(!window.IsLineActive && !window.IsAwaitingFirstPoint, "LINE initial state");
            Check(window.PendingFirstPoint is null && window.LastSnap is null, "LINE initial transients");
            Check(!window.CanUndo && !window.CanRedo, "history initial state");
            Check(window.CurrentPath is null && !window.IsDirty, "document initial state");
            Check(tree.Viewport.Lines.Length == 0 && tree.Viewport.PreviewVertices.Length == 0, "viewport initially empty");
            Check(!tree.Viewport.HasCursor && tree.Viewport.SelectedEntityId is null, "viewport initial overlays");
            Check(window.IsObjectSnapEnabled && tree.OsnapStatus.Classes.Contains("active"),
                "object snap initially enabled");
            Check(!window.IsOrthoEnabled && !tree.OrthoStatus.Classes.Contains("active"),
                "ortho initially disabled");
            Check(window.AreViewportControlsVisible, "viewport controls initially visible");
            Check(!tree.Viewport.ShowDimensions && !tree.Viewport.UseHeavyLineweight,
                "dimension and lineweight overlays initially disabled");
            Check(tree.Viewport.Zoom == 1 && tree.Viewport.Pan == default && !window.IsPanMode,
                "camera initial state");

            var baseFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "drawing-selection", 1, baseFrame);
            SaveDemoFrame(capturePath, "history-persistence", 1, baseFrame);
            Check(baseFrame.PixelSize == new PixelSize(1672, 941), "rendered frame pixel size");
            var viewportPixels = PixelBounds(BoundsIn(tree.Viewport, window));
            CheckInsideFrame(viewportPixels, baseFrame.PixelSize, "viewport pixel bounds");
            var drawingPixels = new PixelRect(
                viewportPixels.X,
                viewportPixels.Y,
                viewportPixels.Width,
                viewportPixels.Height - 100);

            var unavailableLines = window.Lines.ToArray();
            var unavailableSequence = window.LastTransactionSequence;
            var unavailablePath = window.CurrentPath;
            var unavailableDirty = window.IsDirty;
            Click(tree.UnsupportedButton);
            var unavailableStatus = tree.StatusText.Text ?? string.Empty;
            Check(unavailableStatus.StartsWith("No disponible:", StringComparison.Ordinal),
                "unsupported control status");
            Check(!unavailableStatus.Contains("aplicado", StringComparison.OrdinalIgnoreCase) &&
                !unavailableStatus.Contains("creado", StringComparison.OrdinalIgnoreCase) &&
                !unavailableStatus.Contains("ejecutado", StringComparison.OrdinalIgnoreCase),
                "unsupported control has no false success");
            Check(window.Lines.Span.SequenceEqual(unavailableLines) &&
                window.LastTransactionSequence == unavailableSequence &&
                window.CurrentPath == unavailablePath &&
                window.IsDirty == unavailableDirty,
                "unsupported control is non-mutating");

            Click(tree.CloseProperties);
            Check(!tree.PropertiesDock.IsVisible, "properties dock close");
            Click(tree.PropertiesRibbon);
            Check(tree.PropertiesDock.IsVisible, "properties dock open");
            Click(tree.LayerManagerButton);
            Check(tree.LayerManager.IsVisible, "layer manager open");
            Click(tree.LayerRowsToggle);
            Check(!tree.LayerRows.IsVisible, "layer rows collapse");
            Click(tree.LayerRowsToggle);
            Check(tree.LayerRows.IsVisible, "layer rows restore");
            Click(tree.CloseLayers);
            Check(!tree.LayerManager.IsVisible, "layer manager close");
            Click(tree.GeneralSection);
            Check(!tree.GeneralRows.IsVisible, "general properties collapse");
            Click(tree.GeneralSection);
            Check(tree.GeneralRows.IsVisible, "general properties restore");
            Click(tree.ViewSection);
            Check(!tree.ViewRows.IsVisible, "view properties collapse");
            Click(tree.ViewSection);
            Check(tree.ViewRows.IsVisible, "view properties restore");
            Click(tree.MiscSection);
            Check(!tree.MiscRows.IsVisible, "misc properties collapse");
            Click(tree.MiscSection);
            Check(tree.MiscRows.IsVisible, "misc properties restore");
            Click(tree.ViewportMenu);
            Check(!window.AreViewportControlsVisible && Equals(tree.ViewportMenu.Content, "[+]"),
                "viewport controls collapse");
            Click(tree.ViewportMenu);
            Check(window.AreViewportControlsVisible && Equals(tree.ViewportMenu.Content, "[–]"),
                "viewport controls restore");

            var featurePath = Path.Combine(tempDirectory, "house-demo.arcf");
            Click(tree.OsnapStatus);
            Check(!window.IsObjectSnapEnabled, "house demo object snap disabled for exact construction");

            Click(tree.MultilineButton);
            Check(window.IsLineActive && window.ActiveDrawingTool == "Multiline", "multiline route active");
            window.AcceptPoint(new ArcCadPoint(80, 80));
            window.MovePointer(new ArcCadPoint(420, 80));
            Check(tree.Viewport.PreviewVertices.Length == 8, "multiline two-LINE preview");
            window.AcceptPoint(new ArcCadPoint(420, 80));
            window.AcceptPoint(new ArcCadPoint(420, 300));
            window.AcceptPoint(new ArcCadPoint(80, 300));
            window.AcceptPoint(new ArcCadPoint(80, 80));
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "multiline Escape handled");
            Check(window.Lines.Length == 8 && !window.IsLineActive, "four wall segments create eight native LINE");

            var beforeRectangleEntityCount = window.EntityCount;
            var beforeRectangleTransaction = window.LastTransactionSequence!.Value;
            Click(tree.RectangleButton);
            Check(window.ActiveDrawingTool == "Rectangle", "rectangle route active");
            window.AcceptPoint(new ArcCadPoint(100, 100));
            window.MovePointer(new ArcCadPoint(250, 280));
            Check(tree.Viewport.PreviewVertices.Length == 16, "rectangle four-LINE preview");
            window.AcceptPoint(new ArcCadPoint(250, 280));
            var rectangleId = window.LastCreatedEntityId;
            var rectangle = window.Entities.ToArray().Single(entity => entity.EntityId == rectangleId);
            var rectangleVertices = rectangle.Vertices.Span;
            Check(window.Lines.Length == 8 && window.EntityCount == beforeRectangleEntityCount + 1 &&
                window.LastTransactionSequence == beforeRectangleTransaction + 1 && !window.IsLineActive &&
                rectangle.PointCount == 5 && rectangleVertices[0] == rectangleVertices[^2] &&
                rectangleVertices[1] == rectangleVertices[^1],
                "RECTANG native creates one closed polyline in one transaction");
            window.SelectAt(new ArcCadPoint(100, 190));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == rectangleId && tree.PropertyType.Text == "LWPOLYLINE",
                "RECTANG native ID is selectable and typed");
            var rectangleArea = window.MeasureSelectedArea();
            Check(rectangleArea.Contains("Area = 27000", StringComparison.Ordinal) &&
                rectangleArea.Contains("Perimeter = 660", StringComparison.Ordinal),
                "AREA native reports exact RECTANG area and perimeter");
            window.Undo();
            Check(window.EntityCount == beforeRectangleEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != rectangleId),
                "RECTANG native undo removes one entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == rectangleId)
                    .Vertices.Span.SequenceEqual(rectangle.Vertices.Span),
                "RECTANG native redo restores exact ID and geometry");

            var beforePolylineEntityCount = window.EntityCount;
            var beforePolylineLineCount = window.Lines.Length;
            var beforePolylineTransaction = window.LastTransactionSequence!.Value;
            Click(tree.PolylineButton);
            window.AcceptPoint(new ArcCadPoint(240, 100));
            ExpectThrows<ArgumentException>(
                () => window.AcceptPoint(new ArcCadPoint(240, 100)),
                "PLINE rejects a coincident consecutive vertex before native mutation");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None) && !window.IsLineActive &&
                window.EntityCount == beforePolylineEntityCount &&
                window.LastTransactionSequence == beforePolylineTransaction,
                "PLINE one-point cancel creates no entity or transaction");

            Click(tree.PolylineButton);
            Check(window.ActiveDrawingTool == "Polyline", "polyline route active");
            window.AcceptPoint(new ArcCadPoint(250, 100));
            window.MovePointer(new ArcCadPoint(250, 180));
            Check(tree.Viewport.PreviewVertices.Length == 4, "PLINE tentative segment preview");
            window.AcceptPoint(new ArcCadPoint(250, 180));
            window.MovePointer(new ArcCadPoint(400, 180));
            Check(tree.Viewport.PreviewVertices.Length == 8, "PLINE accumulated preview");
            window.AcceptPoint(new ArcCadPoint(400, 180));
            Check(window.EntityCount == beforePolylineEntityCount &&
                window.Lines.Length == beforePolylineLineCount &&
                window.LastTransactionSequence == beforePolylineTransaction,
                "PLINE preview consumes no native ID or transaction");
            Check(window.HandleKey(Key.Enter, KeyModifiers.None), "polyline Enter confirms default");
            var polylineId = window.LastCreatedEntityId;
            var polyline = window.Entities.ToArray().Single(entity => entity.EntityId == polylineId);
            Check(window.Lines.Length == beforePolylineLineCount &&
                window.EntityCount == beforePolylineEntityCount + 1 &&
                window.LastTransactionSequence == beforePolylineTransaction + 1 &&
                polyline.PointCount == 3 && !window.IsLineActive,
                "PLINE native creates one multivertex entity in one transaction");
            window.SelectAt(new ArcCadPoint(325, 180));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == polylineId && window.SelectedLine is null &&
                tree.PropertyType.Text == "LWPOLYLINE" &&
                polyline.AnalyticLength is { } polylineLength &&
                Math.Abs(polylineLength - 230.0) < 0.000000001 &&
                tree.PropertyLength.Text == "230.00" &&
                (AutomationProperties.GetName(tree.Properties) ?? string.Empty).Contains(
                    "longitud 230.00", StringComparison.Ordinal),
                "PLINE native is selectable, typed and analytically measured as one entity");
            window.Undo();
            Check(window.EntityCount == beforePolylineEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != polylineId),
                "PLINE undo removes the complete entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == polylineId)
                    .Vertices.Span.SequenceEqual(polyline.Vertices.Span),
                "PLINE redo restores exact ID and geometry");

            window.SelectAt(new ArcCadPoint(250, 74));
            Check(window.SelectedLine is not null && tree.MoveButton.IsEnabled && tree.RotateButton.IsEnabled &&
                tree.CopyButton.IsEnabled && tree.MirrorButton.IsEnabled && tree.ArrayButton.IsEnabled,
                "selection enables native transforms");

            var sourceBeforeMove = window.SelectedLine!.Value;
            var sourceLineCount = window.Lines.Length;
            var moveTransaction = window.LastTransactionSequence!.Value;
            Click(tree.MoveButton);
            Check(window.ActiveDrawingTool == "Move", "move route active");
            window.AcceptPoint(new ArcCadPoint(250, 74));
            window.MovePointer(new ArcCadPoint(262, 82));
            Check(tree.Viewport.PreviewVertices.Length == 4, "move translated preview");
            window.AcceptPoint(new ArcCadPoint(262, 82));
            var sourceAfterMove = window.SelectedLine!.Value;
            Check(window.Lines.Length == sourceLineCount && sourceAfterMove.EntityId == sourceBeforeMove.EntityId &&
                sourceAfterMove.X1 == sourceBeforeMove.X1 + 12 && sourceAfterMove.Y1 == sourceBeforeMove.Y1 + 8 &&
                sourceAfterMove.X2 == sourceBeforeMove.X2 + 12 && sourceAfterMove.Y2 == sourceBeforeMove.Y2 + 8 &&
                window.LastTransactionSequence == moveTransaction + 1,
                "MOVE native preserves ID and commits one transaction");
            window.Undo();
            Check(window.SelectedLine == sourceBeforeMove, "MOVE native undo restores source");
            window.Redo();
            Check(window.SelectedLine == sourceAfterMove, "MOVE native redo restores translation");
            window.Undo();
            Check(window.SelectedLine == sourceBeforeMove, "MOVE source restored before next transforms");

            var rotateTransaction = window.LastTransactionSequence!.Value;
            Click(tree.RotateButton);
            Check(window.ActiveDrawingTool == "Rotate", "rotate route active");
            window.AcceptPoint(new ArcCadPoint(80, 74));
            window.AcceptPoint(new ArcCadPoint(420, 74));
            window.MovePointer(new ArcCadPoint(80, 414));
            Check(tree.Viewport.PreviewVertices.Span.SequenceEqual([
                80f, 74f, 420f, 74f,
                80f, 74f, 80f, 414f,
            ]), "rotate reference rays preview");
            window.AcceptPoint(new ArcCadPoint(80, 414));
            var sourceAfterRotate = window.SelectedLine!.Value;
            Check(sourceAfterRotate.EntityId == sourceBeforeMove.EntityId &&
                sourceAfterRotate.X1 == 80 && sourceAfterRotate.Y1 == 74 &&
                sourceAfterRotate.X2 == 80 && sourceAfterRotate.Y2 == 414 &&
                window.LastTransactionSequence == rotateTransaction + 1,
                "ROTATE native quarter turn preserves ID and commits one transaction");
            window.Undo();
            Check(window.SelectedLine == sourceBeforeMove, "ROTATE native undo restores source");
            window.Redo();
            Check(window.SelectedLine == sourceAfterRotate, "ROTATE native redo restores quarter turn");
            window.Undo();
            Check(window.SelectedLine == sourceBeforeMove, "ROTATE source restored before COPY");

            var copyTransaction = window.LastTransactionSequence!.Value;
            Click(tree.CopyButton);
            Check(window.ActiveDrawingTool == "Copy", "copy route active");
            window.AcceptPoint(new ArcCadPoint(250, 74));
            window.MovePointer(new ArcCadPoint(320, 74));
            Check(tree.Viewport.PreviewVertices.Length == 4, "copy translated preview");
            window.AcceptPoint(new ArcCadPoint(320, 74));
            Check(window.Lines.Length == sourceLineCount + 1 && !window.IsLineActive &&
                window.LastTransactionSequence == copyTransaction + 1 &&
                window.LineEntityId != sourceBeforeMove.EntityId,
                "COPY native creates one ID in one transaction");

            var mirrorTransaction = window.LastTransactionSequence!.Value;
            Click(tree.MirrorButton);
            Check(window.ActiveDrawingTool == "Mirror", "mirror route active");
            window.AcceptPoint(new ArcCadPoint(300, 90));
            window.MovePointer(new ArcCadPoint(300, 290));
            Check(tree.Viewport.PreviewVertices.Length == 8, "mirror axis and result preview");
            window.AcceptPoint(new ArcCadPoint(300, 290));
            Check(window.Lines.Length == sourceLineCount + 2 && !window.IsLineActive &&
                window.LastTransactionSequence == mirrorTransaction + 1,
                "MIRROR native creates one ID in one transaction");

            var arrayTransaction = window.LastTransactionSequence!.Value;
            Click(tree.ArrayButton);
            Check(window.Lines.Length == sourceLineCount + 7 && !window.IsLineActive &&
                window.LastTransactionSequence == arrayTransaction + 1,
                "ARRAY native creates five IDs in one transaction");

            var beforeMissingOopsCount = window.EntityCount;
            var beforeMissingOopsTransaction = window.LastTransactionSequence;
            EnterCommand(window, "OOPS");
            Check(window.IsBackendConnected && window.EntityCount == beforeMissingOopsCount &&
                window.LastTransactionSequence == beforeMissingOopsTransaction &&
                (tree.StatusText.Text ?? string.Empty).Contains(
                    "no hay ningún ERASE", StringComparison.Ordinal),
                "OOPS without prior ERASE is non-mutating and keeps the backend alive");

            var erasedEntityId = window.LineEntityId;
            var eraseLine = window.Lines.ToArray().Single(line => line.EntityId == erasedEntityId);
            window.SelectAt(new ArcCadPoint(
                (eraseLine.X1 + eraseLine.X2) / 2,
                (eraseLine.Y1 + eraseLine.Y2) / 2));
            Check(window.SelectedEntityId == erasedEntityId, "select latest ARRAY copy for ERASE");
            var eraseTransaction = window.LastTransactionSequence!.Value;
            Check(window.HandleKey(Key.Delete, KeyModifiers.None), "Delete handled by ERASE");
            Check(window.Lines.Length == sourceLineCount + 6 && window.SelectedEntityId is null &&
                window.Lines.ToArray().All(line => line.EntityId != erasedEntityId) &&
                window.LastTransactionSequence == eraseTransaction + 1,
                "ERASE native removes selected ID in one transaction");
            window.Undo();
            Check(window.Lines.Length == sourceLineCount + 7 && window.Lines.ToArray().Any(line => line.EntityId == erasedEntityId),
                "ERASE native undo restores same ID");
            window.Redo();
            Check(window.Lines.Length == sourceLineCount + 6 && window.Lines.ToArray().All(line => line.EntityId != erasedEntityId),
                "ERASE native redo removes same ID");
            window.Undo();
            Check(window.Lines.Length == sourceLineCount + 7 && window.Lines.ToArray().Any(line => line.EntityId == erasedEntityId),
                "ERASE source restored before continuing house");

            window.SelectAt(new ArcCadPoint(
                (eraseLine.X1 + eraseLine.X2) / 2,
                (eraseLine.Y1 + eraseLine.Y2) / 2));
            tree.CommandInput.Text = "E";
            tree.CommandInput.Focus();
            window.KeyPressQwerty(PhysicalKey.Enter, RawInputModifiers.None);
            Dispatcher.UIThread.RunJobs();
            Check(window.Lines.Length == sourceLineCount + 6 && window.Lines.ToArray().All(line => line.EntityId != erasedEntityId),
                "command E reaches native ERASE");

            var laterLineId = CreateLine(
                window,
                new ArcCadPoint(780, 300),
                new ArcCadPoint(790, 310));
            var laterLine = window.Lines.ToArray().Single(line => line.EntityId == laterLineId);
            var beforeOopsTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "OOPS");
            var restoredEntityId = window.LastCreatedEntityId;
            var restoredLine = window.Lines.ToArray()
                .Single(line => line.EntityId == restoredEntityId);
            Check(window.IsBackendConnected && restoredEntityId != erasedEntityId &&
                restoredEntityId != laterLineId &&
                restoredLine.X1 == eraseLine.X1 && restoredLine.Y1 == eraseLine.Y1 &&
                restoredLine.X2 == eraseLine.X2 && restoredLine.Y2 == eraseLine.Y2 &&
                FindLine(window, laterLineId) == laterLine &&
                window.Lines.Length == sourceLineCount + 8 &&
                window.LastTransactionSequence == beforeOopsTransaction + 1 &&
                (tree.StatusText.Text ?? string.Empty).Contains("OOPS nativo", StringComparison.Ordinal),
                "OOPS restores ERASE geometry with a new ID without undoing a later LINE");
            window.SelectAt(new ArcCadPoint(
                (restoredLine.X1 + restoredLine.X2) / 2,
                (restoredLine.Y1 + restoredLine.Y2) / 2));
            Check(window.SelectedEntityId == restoredEntityId,
                "OOPS restored LINE is independently selectable");
            Dispatcher.UIThread.RunJobs();
            window.Undo();
            Check(window.Lines.Length == sourceLineCount + 7 &&
                window.Lines.ToArray().All(line => line.EntityId != restoredEntityId) &&
                FindLine(window, laterLineId) == laterLine,
                "OOPS undo removes only the restored copy");
            window.Undo();
            Check(window.Lines.Length == sourceLineCount + 6 &&
                window.Lines.ToArray().All(line => line.EntityId != laterLineId),
                "next undo removes the command that followed ERASE");
            window.Undo();
            Check(window.Lines.Length == sourceLineCount + 7 &&
                FindLine(window, erasedEntityId) == eraseLine,
                "next undo restores the original ERASE ID");

            var beforeCircleEntityCount = window.EntityCount;
            var beforeCircleLineCount = window.Lines.Length;
            var beforeCircleTransaction = window.LastTransactionSequence!.Value;
            Click(tree.CircleButton);
            Check(window.ActiveDrawingTool == "Circle", "circle route active");
            window.AcceptPoint(new ArcCadPoint(330, 235));
            window.MovePointer(new ArcCadPoint(350, 235));
            Check(tree.Viewport.PreviewVertices.Length == 96, "circle 24-LINE preview");
            window.AcceptPoint(new ArcCadPoint(350, 235));
            var circleId = window.LastCreatedEntityId;
            var circle = window.Entities.ToArray().Single(entity => entity.EntityId == circleId);
            var circleVertices = circle.Vertices.Span;
            Check(window.Lines.Length == beforeCircleLineCount && window.EntityCount == beforeCircleEntityCount + 1 &&
                window.LastTransactionSequence == beforeCircleTransaction + 1 && !window.IsLineActive &&
                circle.PointCount > 8 &&
                circleVertices[0] == circleVertices[^2] && circleVertices[1] == circleVertices[^1],
                "CIRCLE native creates one closed entity in one transaction");
            window.SelectAt(new ArcCadPoint(350, 235));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == circleId && tree.Viewport.SelectedEntityId == circleId &&
                tree.PropertyType.Text == "CIRCLE" && tree.PropertyId.Text == circleId.ToString() &&
                circle.AnalyticLength is { } circleLength &&
                Math.Abs(circleLength - 125.66370614359172) < 0.000000001 &&
                Math.Abs(circle.VisibleLength - circleLength) > 0.01 &&
                tree.PropertyLength.Text == "125.66" &&
                AutomationProperties.GetName(tree.Properties) ==
                    $"Propiedades CIRCLE, ID {circleId}, longitud 125.66",
                "CIRCLE native ID is selectable, typed and uses analytic circumference");
            window.Undo();
            Check(window.EntityCount == beforeCircleEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != circleId),
                "CIRCLE native undo removes one entity");
            window.Redo();
            var redoneCircle = window.Entities.ToArray().Single(entity => entity.EntityId == circleId);
            Check(window.EntityCount == beforeCircleEntityCount + 1 &&
                redoneCircle.Vertices.Span.SequenceEqual(circle.Vertices.Span) &&
                redoneCircle.AnalyticLength == circle.AnalyticLength,
                "CIRCLE native redo restores the same ID, geometry and analytic length");
            var circleRadiusEntityCount = window.EntityCount;
            var circleRadiusLineCount = window.Lines.Length;
            var circleRadiusTransaction = window.LastTransactionSequence;
            var circleRadiusCanUndo = window.CanUndo;
            var circleRadiusCanRedo = window.CanRedo;
            window.SelectAt(new ArcCadPoint(760, 280));
            Check(window.SelectedEntityId is null, "MEASUREGEOM Radius no-selection setup");
            EnterCommand(window, "MEA RADIUS");
            Check(window.IsBackendConnected && window.EntityCount == circleRadiusEntityCount &&
                window.Lines.Length == circleRadiusLineCount &&
                window.LastTransactionSequence == circleRadiusTransaction &&
                window.CanUndo == circleRadiusCanUndo && window.CanRedo == circleRadiusCanRedo &&
                (tree.StatusText.Text ?? string.Empty).Contains("Seleccione", StringComparison.Ordinal),
                "MEASUREGEOM Radius without selection is view-only");
            window.SelectAt(new ArcCadPoint(350, 235));
            EnterCommand(window, "MEA   RADIUS");
            var circleRadius = tree.StatusText.Text ?? string.Empty;
            Check(window.SelectedEntityId == circleId &&
                circleRadius.Contains("Radius = 20", StringComparison.Ordinal) &&
                circleRadius.Contains("Diameter = 40", StringComparison.Ordinal) &&
                circleRadius.Contains("Circumference = 125.6637", StringComparison.Ordinal) &&
                circleRadius.Contains("Area = 1256.6371", StringComparison.Ordinal) &&
                window.EntityCount == circleRadiusEntityCount &&
                window.LastTransactionSequence == circleRadiusTransaction &&
                window.Entities.ToArray().Single(entity => entity.EntityId == circleId)
                    .Vertices.Span.SequenceEqual(circle.Vertices.Span),
                "MEASUREGEOM Radius reports exact CIRCLE mathematics without mutation");

            var beforeArcEntityCount = window.EntityCount;
            var beforeArcLineCount = window.Lines.Length;
            var beforeArcTransaction = window.LastTransactionSequence!.Value;
            Click(tree.ArcButton);
            Check(window.ActiveDrawingTool == "Arc", "arc route active");
            window.AcceptPoint(new ArcCadPoint(250, 100));
            window.AcceptPoint(new ArcCadPoint(270, 120));
            window.MovePointer(new ArcCadPoint(290, 100));
            Check(tree.Viewport.PreviewVertices.Length >= 24, "three-point arc segmented preview");
            window.AcceptPoint(new ArcCadPoint(290, 100));
            var arcId = window.LastCreatedEntityId;
            var arc = window.Entities.ToArray().Single(entity => entity.EntityId == arcId);
            var arcVertices = arc.Vertices.Span;
            Check(window.Lines.Length == beforeArcLineCount && window.EntityCount == beforeArcEntityCount + 1 &&
                window.LastTransactionSequence == beforeArcTransaction + 1 && !window.IsLineActive &&
                arc.PointCount > 3 &&
                (arcVertices[0] != arcVertices[^2] || arcVertices[1] != arcVertices[^1]),
                "ARC native creates one open entity in one transaction");
            window.SelectAt(new ArcCadPoint(270, 120));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == arcId && tree.Viewport.SelectedEntityId == arcId &&
                tree.PropertyType.Text == "ARC" && tree.PropertyId.Text == arcId.ToString() &&
                arc.AnalyticLength is { } arcLength &&
                Math.Abs(arcLength - 62.83185307179586) < 0.000000001 &&
                Math.Abs(arc.VisibleLength - arcLength) > 0.001 &&
                tree.PropertyLength.Text == "62.83" &&
                AutomationProperties.GetName(tree.Properties) ==
                    $"Propiedades ARC, ID {arcId}, longitud 62.83",
                "ARC native ID is selectable, typed and uses analytic arc length");
            window.Undo();
            Check(window.EntityCount == beforeArcEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != arcId),
                "ARC native undo removes one entity");
            window.Redo();
            var redoneArc = window.Entities.ToArray().Single(entity => entity.EntityId == arcId);
            Check(window.EntityCount == beforeArcEntityCount + 1 &&
                redoneArc.Vertices.Span.SequenceEqual(arc.Vertices.Span) &&
                redoneArc.AnalyticLength == arc.AnalyticLength,
                "ARC native redo restores the same ID, geometry and analytic length");
            var arcRadiusTransaction = window.LastTransactionSequence;
            window.SelectAt(new ArcCadPoint(270, 120));
            EnterCommand(window, "MEASUREGEOM RADIUS");
            var arcRadius = tree.StatusText.Text ?? string.Empty;
            Check(window.SelectedEntityId == arcId &&
                arcRadius.Contains("Radius = 20", StringComparison.Ordinal) &&
                arcRadius.Contains("Diameter = 40", StringComparison.Ordinal) &&
                arcRadius.Contains("Arc length = 62.8319", StringComparison.Ordinal) &&
                window.LastTransactionSequence == arcRadiusTransaction &&
                window.Entities.ToArray().Single(entity => entity.EntityId == arcId)
                    .Vertices.Span.SequenceEqual(arc.Vertices.Span),
                "MEASUREGEOM Radius reports exact ARC mathematics without mutation");
            window.SelectAt(new ArcCadPoint(250, 74));
            Check(tree.PropertyType.Text == "LINE", "MEASUREGEOM Radius invalid-type setup");
            EnterCommand(window, "MEA RADIUS");
            Check(window.IsBackendConnected && window.LastTransactionSequence == arcRadiusTransaction &&
                window.EntityCount == beforeArcEntityCount + 1 &&
                (tree.StatusText.Text ?? string.Empty).Contains(
                    "ninguna entidad circular", StringComparison.Ordinal),
                "MEASUREGEOM Radius rejects LINE without mutation or session loss");

            var beforeEllipseEntityCount = window.EntityCount;
            var beforeEllipseLineCount = window.Lines.Length;
            var beforeEllipseTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "EL");
            Check(window.ActiveDrawingTool == "Ellipse", "ELLIPSE alias route active");
            window.AcceptPoint(new ArcCadPoint(330, 130));
            window.AcceptPoint(new ArcCadPoint(390, 130));
            ExpectThrows<ArgumentException>(
                () => window.AcceptPoint(new ArcCadPoint(360, 130)),
                "ELLIPSE rejects a zero perpendicular semi-axis before mutation");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None) &&
                window.EntityCount == beforeEllipseEntityCount &&
                window.LastTransactionSequence == beforeEllipseTransaction,
                "ELLIPSE degenerate cancel creates no entity or transaction");

            EnterCommand(window, "ELLIPSE");
            window.AcceptPoint(new ArcCadPoint(330, 130));
            window.MovePointer(new ArcCadPoint(390, 130));
            Check(tree.Viewport.PreviewVertices.Length == 4, "ELLIPSE major-axis preview");
            window.AcceptPoint(new ArcCadPoint(390, 130));
            window.MovePointer(new ArcCadPoint(330, 160));
            Check(tree.Viewport.PreviewVertices.Length == 96 &&
                window.EntityCount == beforeEllipseEntityCount &&
                window.LastTransactionSequence == beforeEllipseTransaction,
                "ELLIPSE closed preview is ephemeral");
            window.AcceptPoint(new ArcCadPoint(330, 160));
            var ellipseId = window.LastCreatedEntityId;
            var ellipse = window.Entities.ToArray().Single(entity => entity.EntityId == ellipseId);
            var ellipseCoordinates = ellipse.Vertices.ToArray();
            var ellipseXs = ellipseCoordinates.Where((_, index) => (index & 1) == 0).ToArray();
            var ellipseYs = ellipseCoordinates.Where((_, index) => (index & 1) != 0).ToArray();
            Check(window.Lines.Length == beforeEllipseLineCount &&
                window.EntityCount == beforeEllipseEntityCount + 1 &&
                window.LastTransactionSequence == beforeEllipseTransaction + 1 &&
                ellipse.PointCount > 8 &&
                Math.Abs(ellipseXs.Min() - 270) < 0.15 && Math.Abs(ellipseXs.Max() - 390) < 0.15 &&
                Math.Abs(ellipseYs.Min() - 100) < 0.15 && Math.Abs(ellipseYs.Max() - 160) < 0.15,
                $"ELLIPSE native creates exact 60x30 semi-axes in one entity and transaction " +
                $"(x={ellipseXs.Min():F4}..{ellipseXs.Max():F4}, y={ellipseYs.Min():F4}..{ellipseYs.Max():F4})");
            window.SelectAt(new ArcCadPoint(390, 130));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == ellipseId && tree.PropertyType.Text == "ELLIPSE",
                "ELLIPSE native ID is selectable and typed");
            Check(ellipse.AnalyticLength is { } ellipseLength &&
                Math.Abs(ellipseLength - 290.653446616445) < 0.000000001 &&
                tree.PropertyLength.Text == "290.65" &&
                (AutomationProperties.GetName(tree.Properties) ?? string.Empty).Contains(
                    "longitud 290.65", StringComparison.Ordinal),
                "ELLIPSE Properties uses the model analytic perimeter");
            window.ListSelectedEntity();
            var ellipseList = tree.StatusText.Text ?? string.Empty;
            Check(ellipseList.Contains("center 330.0000,130.0000", StringComparison.Ordinal) &&
                ellipseList.Contains("semi-major 60.0000", StringComparison.Ordinal) &&
                ellipseList.Contains("semi-minor 30.0000", StringComparison.Ordinal) &&
                ellipseList.Contains("ratio 0.5", StringComparison.Ordinal),
                "ELLIPSE LIST reports exact native center, axes and ratio");
            var ellipseAreaTransaction = window.LastTransactionSequence;
            var ellipseArea = window.MeasureSelectedArea();
            Check(ellipseArea.Contains("Area = 5654.8668", StringComparison.Ordinal) &&
                ellipseArea.Contains("Perimeter = 290.6534", StringComparison.Ordinal) &&
                window.LastTransactionSequence == ellipseAreaTransaction &&
                window.Entities.ToArray().Single(entity => entity.EntityId == ellipseId)
                    .Vertices.Span.SequenceEqual(ellipse.Vertices.Span),
                "AREA computes exact ELLIPSE area/perimeter without mutation");
            EnterCommand(window, "AA");
            Check((tree.StatusText.Text ?? string.Empty).Contains(
                    "Total area = 5654.8668", StringComparison.Ordinal),
                "AA command reaches native ELLIPSE AREA");
            Click(ActionButton(window, "ribbon.tab.herramientas"));
            var ellipseAreaButton = ActionButton(window, "ribbon.herramientas.area");
            Click(ellipseAreaButton);
            Check((tree.StatusText.Text ?? string.Empty).Contains(
                    "Total area = 5654.8668", StringComparison.Ordinal),
                "AREA ribbon action reaches native ELLIPSE calculation");
            Click(ActionButton(window, "ribbon.tab.inicio"));
            window.Undo();
            Check(window.EntityCount == beforeEllipseEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != ellipseId),
                "ELLIPSE native undo removes one entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == ellipseId)
                    .Vertices.Span.SequenceEqual(ellipse.Vertices.Span),
                "ELLIPSE native redo restores exact ID and geometry");

            var beforeEllipseCenterEntityCount = window.EntityCount;
            var beforeEllipseCenterTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "ELLIPSE C 0");
            Check(window.ActiveDrawingTool == "None" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Uso: ELLIPSE C", StringComparison.Ordinal) &&
                window.EntityCount == beforeEllipseCenterEntityCount &&
                window.LastTransactionSequence == beforeEllipseCenterTransaction,
                "ELLIPSE C rejects an invalid ratio without starting or mutating");
            EnterCommand(window, "EL C 0.5");
            Check(window.ActiveDrawingTool == "EllipseCenter", "EL C alias route active");
            var ellipseCenterPoint = new ArcCadPoint(1260, 300);
            var ellipseCenterPick = new ArcCadPoint(1300, 300);
            window.AcceptPoint(ellipseCenterPoint);
            ExpectThrows<ArgumentException>(
                () => window.AcceptPoint(ellipseCenterPoint),
                "ELLIPSE C rejects a zero major axis");
            Check(window.IsBackendConnected && window.ActiveDrawingTool == "EllipseCenter" &&
                window.EntityCount == beforeEllipseCenterEntityCount &&
                window.LastTransactionSequence == beforeEllipseCenterTransaction,
                "ELLIPSE C degenerate axis is retryable with no transaction or session loss");
            window.AcceptPoint(ellipseCenterPick);
            var ellipseCenterId = window.LastCreatedEntityId;
            var ellipseCenter = window.Entities.ToArray()
                .Single(entity => entity.EntityId == ellipseCenterId);
            var ellipseCenterCoordinates = ellipseCenter.Vertices.ToArray();
            var ellipseCenterXs = ellipseCenterCoordinates.Where((_, index) => (index & 1) == 0).ToArray();
            var ellipseCenterYs = ellipseCenterCoordinates.Where((_, index) => (index & 1) != 0).ToArray();
            Check(window.EntityCount == beforeEllipseCenterEntityCount + 1 &&
                window.LastTransactionSequence == beforeEllipseCenterTransaction + 1 &&
                ellipseCenter.PointCount > 8 &&
                Math.Abs(ellipseCenterXs.Min() - 1220) < 0.15 &&
                Math.Abs(ellipseCenterXs.Max() - 1300) < 0.15 &&
                Math.Abs(ellipseCenterYs.Min() - 280) < 0.15 &&
                Math.Abs(ellipseCenterYs.Max() - 320) < 0.15,
                "ELLIPSE C creates exact 40x20 semi-axes in one entity and transaction");
            window.SelectAt(ellipseCenterPick);
            window.ListSelectedEntity();
            var ellipseCenterList = tree.StatusText.Text ?? string.Empty;
            Check(window.SelectedEntityId == ellipseCenterId && tree.PropertyType.Text == "ELLIPSE" &&
                ellipseCenter.AnalyticLength is { } ellipseCenterLength &&
                Math.Abs(ellipseCenterLength - 193.768964410963) < 0.000000001 &&
                tree.PropertyLength.Text == "193.77" &&
                ellipseCenterList.Contains("center 1260.0000,300.0000", StringComparison.Ordinal) &&
                ellipseCenterList.Contains("semi-major 40.0000", StringComparison.Ordinal) &&
                ellipseCenterList.Contains("semi-minor 20.0000", StringComparison.Ordinal) &&
                ellipseCenterList.Contains("ratio 0.5", StringComparison.Ordinal),
                "ELLIPSE C LIST reports native center, axes and ratio");
            var ellipseCenterArea = window.MeasureSelectedArea();
            Check(ellipseCenterArea.Contains("Area = 2513.2741", StringComparison.Ordinal) &&
                ellipseCenterArea.Contains("Perimeter = 193.7690", StringComparison.Ordinal) &&
                window.LastTransactionSequence == beforeEllipseCenterTransaction + 1,
                "ELLIPSE C AREA reports exact area and Simpson perimeter without mutation");
            EnterCommand(window, "MEASUREGEOM LENGTH");
            Check((tree.StatusText.Text ?? string.Empty).Contains("Length = 193.7690", StringComparison.Ordinal) &&
                window.LastTransactionSequence == beforeEllipseCenterTransaction + 1 &&
                window.Entities.ToArray().Single(entity => entity.EntityId == ellipseCenterId)
                    .Vertices.Span.SequenceEqual(ellipseCenter.Vertices.Span),
                "MEASUREGEOM LENGTH reports full ELLIPSE independently of tessellation");
            window.Undo();
            Check(window.EntityCount == beforeEllipseCenterEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != ellipseCenterId),
                "ELLIPSE C undo removes one entity");
            window.Redo();
            var redoneEllipseCenter = window.Entities.ToArray()
                .Single(entity => entity.EntityId == ellipseCenterId);
            Check(redoneEllipseCenter.Vertices.Span.SequenceEqual(ellipseCenter.Vertices.Span) &&
                redoneEllipseCenter.AnalyticLength == ellipseCenter.AnalyticLength,
                "ELLIPSE C redo restores exact ID, geometry and analytic length");

            var beforeEllipseArcEntityCount = window.EntityCount;
            var beforeEllipseArcTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "ELLIPSE ARC 0.5 0");
            Check(window.ActiveDrawingTool == "None" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Uso: ELLIPSE ARC", StringComparison.Ordinal) &&
                window.EntityCount == beforeEllipseArcEntityCount &&
                window.LastTransactionSequence == beforeEllipseArcTransaction,
                "ELLIPSE ARC rejects incomplete syntax without mutation");
            EnterCommand(window, "EL ARC 0.5 0 90");
            Check(window.ActiveDrawingTool == "EllipseArc", "EL ARC alias route active");
            var ellipseArcCenter = new ArcCadPoint(1360, 300);
            var ellipseArcAxisEnd = new ArcCadPoint(1400, 300);
            window.AcceptPoint(ellipseArcCenter);
            ExpectThrows<ArgumentException>(
                () => window.AcceptPoint(ellipseArcCenter),
                "ELLIPSE ARC rejects a zero major axis");
            Check(window.IsBackendConnected && window.ActiveDrawingTool == "EllipseArc" &&
                window.EntityCount == beforeEllipseArcEntityCount &&
                window.LastTransactionSequence == beforeEllipseArcTransaction,
                "ELLIPSE ARC degenerate axis is retryable and non-mutating");
            window.AcceptPoint(ellipseArcAxisEnd);
            var ellipseArcId = window.LastCreatedEntityId;
            var ellipseArc = window.Entities.ToArray().Single(entity => entity.EntityId == ellipseArcId);
            var ellipseArcCoordinates = ellipseArc.Vertices.Span;
            Check(window.EntityCount == beforeEllipseArcEntityCount + 1 &&
                window.LastTransactionSequence == beforeEllipseArcTransaction + 1 &&
                ellipseArc.PointCount > 2 &&
                Math.Abs(ellipseArcCoordinates[0] - 1400) < 0.001 &&
                Math.Abs(ellipseArcCoordinates[1] - 300) < 0.001 &&
                Math.Abs(ellipseArcCoordinates[^2] - 1360) < 0.001 &&
                Math.Abs(ellipseArcCoordinates[^1] - 320) < 0.001 &&
                ellipseArc.AnalyticLength is { } ellipseArcLength &&
                Math.Abs(ellipseArcLength - 48.442241102741) < 0.000000001,
                "ELLIPSE ARC creates one exact CCW quarter in one native transaction");
            var ellipseArcPick = new ArcCadPoint(1388.284271247462, 314.142135623731);
            window.SelectAt(ellipseArcPick);
            window.ListSelectedEntity();
            var ellipseArcList = tree.StatusText.Text ?? string.Empty;
            Check(window.SelectedEntityId == ellipseArcId && tree.PropertyType.Text == "ELLIPSE" &&
                tree.PropertyLength.Text == "48.44" &&
                ellipseArcList.Contains("center 1360.0000,300.0000", StringComparison.Ordinal) &&
                ellipseArcList.Contains("semi-major 40.0000", StringComparison.Ordinal) &&
                ellipseArcList.Contains("semi-minor 20.0000", StringComparison.Ordinal) &&
                ellipseArcList.Contains("start 0.0000°", StringComparison.Ordinal) &&
                ellipseArcList.Contains("end 90.0000°", StringComparison.Ordinal),
                "ELLIPSE ARC LIST reports native axes and parameter sweep");
            var ellipseArcArea = window.MeasureSelectedArea();
            Check(ellipseArcArea.Contains("no es un área cerrada", StringComparison.Ordinal) &&
                ellipseArcArea.Contains("Total area = 0.0000", StringComparison.Ordinal) &&
                window.LastTransactionSequence == beforeEllipseArcTransaction + 1,
                "AREA correctly omits the open ELLIPSE ARC without mutation");
            EnterCommand(window, "MEA LENGTH");
            Check((tree.StatusText.Text ?? string.Empty).Contains("Length = 48.4422", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Total length = 48.4422", StringComparison.Ordinal) &&
                window.LastTransactionSequence == beforeEllipseArcTransaction + 1 &&
                window.Entities.ToArray().Single(entity => entity.EntityId == ellipseArcId)
                    .Vertices.Span.SequenceEqual(ellipseArc.Vertices.Span),
                "MEA LENGTH integrates the ELLIPSE ARC sweep without mutation");
            var boundsCanUndo = window.CanUndo;
            var boundsCanRedo = window.CanRedo;
            EnterCommand(window, "MEA BOUNDS");
            var ellipseArcBounds = tree.StatusText.Text ?? string.Empty;
            Check(ellipseArcBounds.Contains("Min = 1360.0000,300.0000", StringComparison.Ordinal) &&
                ellipseArcBounds.Contains("Max = 1400.0000,320.0000", StringComparison.Ordinal) &&
                ellipseArcBounds.Contains("Width = 40.0000", StringComparison.Ordinal) &&
                ellipseArcBounds.Contains("Height = 20.0000", StringComparison.Ordinal) &&
                window.LastTransactionSequence == beforeEllipseArcTransaction + 1 &&
                window.CanUndo == boundsCanUndo && window.CanRedo == boundsCanRedo &&
                window.Entities.ToArray().Single(entity => entity.EntityId == ellipseArcId)
                    .Vertices.Span.SequenceEqual(ellipseArc.Vertices.Span),
                $"MEA BOUNDS reports the native ELLIPSE ARC box without mutation; actual={ellipseArcBounds}");
            window.SelectAt(new ArcCadPoint(1600, 500));
            EnterCommand(window, "MEA LENGTH");
            Check(window.SelectedEntityId is null && window.IsBackendConnected &&
                window.LastTransactionSequence == beforeEllipseArcTransaction + 1 &&
                (tree.StatusText.Text ?? string.Empty).Contains("Seleccione una entidad", StringComparison.Ordinal),
                "MEA LENGTH without selection is non-mutating and keeps the session alive");
            EnterCommand(window, "MEA BOUNDS");
            Check(window.SelectedEntityId is null && window.IsBackendConnected &&
                window.LastTransactionSequence == beforeEllipseArcTransaction + 1 &&
                window.CanUndo == boundsCanUndo && window.CanRedo == boundsCanRedo &&
                (tree.StatusText.Text ?? string.Empty).Contains("Seleccione una entidad", StringComparison.Ordinal),
                "MEA BOUNDS without selection is non-mutating and keeps the session alive");
            window.SelectAt(ellipseArcPick);
            window.Undo();
            Check(window.EntityCount == beforeEllipseArcEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != ellipseArcId),
                "ELLIPSE ARC undo removes one entity");
            window.Redo();
            var redoneEllipseArc = window.Entities.ToArray()
                .Single(entity => entity.EntityId == ellipseArcId);
            Check(redoneEllipseArc.Vertices.Span.SequenceEqual(ellipseArc.Vertices.Span) &&
                redoneEllipseArc.AnalyticLength == ellipseArc.AnalyticLength,
                "ELLIPSE ARC redo restores exact ID, geometry and analytic length");

            var beforePolygonEntityCount = window.EntityCount;
            var beforePolygonLineCount = window.Lines.Length;
            var beforePolygonTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "POLYGON 6");
            Check(window.ActiveDrawingTool == "Polygon", "POLYGON command route active");
            window.AcceptPoint(new ArcCadPoint(150, 200));
            window.MovePointer(new ArcCadPoint(175, 200));
            Check(tree.Viewport.PreviewVertices.Length == 24, "POLYGON six-side preview");
            window.AcceptPoint(new ArcCadPoint(175, 200));
            var polygonId = window.LastCreatedEntityId;
            var polygon = window.Entities.ToArray().Single(entity => entity.EntityId == polygonId);
            var polygonVertices = polygon.Vertices.Span;
            Check(window.Lines.Length == beforePolygonLineCount &&
                window.EntityCount == beforePolygonEntityCount + 1 &&
                window.LastTransactionSequence == beforePolygonTransaction + 1 &&
                polygon.PointCount == 7 && polygonVertices[0] == polygonVertices[^2] &&
                polygonVertices[1] == polygonVertices[^1],
                "POLYGON native creates one closed polyline in one transaction");
            window.SelectAt(new ArcCadPoint(175, 200));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == polygonId && tree.PropertyType.Text == "LWPOLYLINE",
                "POLYGON native ID is selectable and typed");
            var polygonArea = window.MeasureSelectedArea();
            Check(polygonArea.Contains("Area = 1623.7976", StringComparison.Ordinal) &&
                polygonArea.Contains("Perimeter = 150", StringComparison.Ordinal),
                "AREA native reports regular hexagon area and perimeter");
            EnterCommand(window, "AA");
            Check((tree.StatusText.Text ?? string.Empty).Contains("Total area = 1623.7976", StringComparison.Ordinal),
                "AA command reaches native AREA");
            Click(ActionButton(window, "ribbon.tab.herramientas"));
            var areaButton = ActionButton(window, "ribbon.herramientas.area");
            Check(areaButton.IsEnabled, "AREA ribbon action enabled for selected polygon");
            Click(areaButton);
            Check((tree.StatusText.Text ?? string.Empty).Contains("Perimeter = 150", StringComparison.Ordinal),
                "AREA ribbon action reaches native calculation");
            Click(ActionButton(window, "ribbon.tab.inicio"));
            window.Undo();
            Check(window.EntityCount == beforePolygonEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != polygonId),
                "POLYGON native undo removes one entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == polygonId)
                    .Vertices.Span.SequenceEqual(polygon.Vertices.Span),
                "POLYGON native redo restores exact ID and geometry");

            var beforeSplineEntityCount = window.EntityCount;
            var beforeSplineLineCount = window.Lines.Length;
            var beforeSplineTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "SPL");
            Check(window.ActiveDrawingTool == "Spline", "SPL alias route active");
            window.AcceptPoint(new ArcCadPoint(100, 250));
            ExpectThrows<ArgumentException>(
                () => window.AcceptPoint(new ArcCadPoint(100, 250)),
                "SPLINE rejects a coincident consecutive fit point before native mutation");
            window.AcceptPoint(new ArcCadPoint(120, 260));
            Check(window.HandleKey(Key.Escape, KeyModifiers.None) && !window.IsLineActive &&
                window.EntityCount == beforeSplineEntityCount &&
                window.LastTransactionSequence == beforeSplineTransaction,
                "SPLINE two-point cancel creates no entity or transaction");

            EnterCommand(window, "SPLINE");
            Check(window.ActiveDrawingTool == "Spline", "SPLINE command route active");
            window.AcceptPoint(new ArcCadPoint(100, 250));
            window.MovePointer(new ArcCadPoint(140, 270));
            Check(tree.Viewport.PreviewVertices.Length == 4, "SPLINE first tentative control segment preview");
            window.AcceptPoint(new ArcCadPoint(140, 270));
            window.MovePointer(new ArcCadPoint(190, 240));
            Check(tree.Viewport.PreviewVertices.Length == 8, "SPLINE accumulated control preview");
            window.AcceptPoint(new ArcCadPoint(190, 240));
            window.MovePointer(new ArcCadPoint(230, 260));
            Check(tree.Viewport.PreviewVertices.Length == 12, "SPLINE fourth-point tentative preview");
            window.AcceptPoint(new ArcCadPoint(230, 260));
            Check(window.EntityCount == beforeSplineEntityCount &&
                window.Lines.Length == beforeSplineLineCount &&
                window.LastTransactionSequence == beforeSplineTransaction &&
                tree.Viewport.PreviewVertices.Length == 12,
                "SPLINE preview consumes no native ID or transaction");
            Check(window.HandleKey(Key.Enter, KeyModifiers.None), "SPLINE Enter confirms default");
            var splineId = window.LastCreatedEntityId;
            var spline = window.Entities.ToArray().Single(entity => entity.EntityId == splineId);
            Check(window.Lines.Length == beforeSplineLineCount &&
                window.EntityCount == beforeSplineEntityCount + 1 &&
                window.LastTransactionSequence == beforeSplineTransaction + 1 &&
                spline.PointCount > 4 && !window.IsLineActive,
                "SPLINE native creates one interpolated entity in one transaction");
            window.SelectAt(new ArcCadPoint(140, 270));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == splineId && window.SelectedLine is null &&
                tree.PropertyType.Text == "SPLINE",
                "SPLINE native is selectable and typed as one entity");
            window.ListSelectedEntity();
            var splineList = tree.StatusText.Text ?? string.Empty;
            Check(splineList.Contains("fit points 4", StringComparison.Ordinal) &&
                splineList.Contains("closed false", StringComparison.Ordinal),
                "SPLINE LIST reports native fit-point count and open state");
            window.Undo();
            Check(window.EntityCount == beforeSplineEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != splineId),
                "SPLINE native undo removes one entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == splineId)
                    .Vertices.Span.SequenceEqual(spline.Vertices.Span),
                "SPLINE native redo restores exact ID and geometry");

            var beforeDonutEntityCount = window.EntityCount;
            var beforeDonutLineCount = window.Lines.Length;
            var beforeDonutTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "DONUT 10 10");
            Check(!window.IsLineActive && window.EntityCount == beforeDonutEntityCount &&
                window.LastTransactionSequence == beforeDonutTransaction &&
                (tree.StatusText.Text ?? string.Empty).Contains("Uso: DONUT", StringComparison.Ordinal),
                "DONUT rejects equal diameters without mutation");

            EnterCommand(window, "DO 6 10");
            Check(window.ActiveDrawingTool == "Donut", "DO alias ring route active");
            window.MovePointer(new ArcCadPoint(310, 250));
            Check(tree.Viewport.PreviewVertices.Length == 96 &&
                window.EntityCount == beforeDonutEntityCount,
                "DONUT exterior preview is ephemeral");
            window.AcceptPoint(new ArcCadPoint(310, 250));
            var donutId = window.LastCreatedEntityId;
            var donut = window.Entities.ToArray().Single(entity => entity.EntityId == donutId);
            var donutCoordinates = donut.Vertices.ToArray();
            var donutXs = donutCoordinates.Where((_, index) => (index & 1) == 0).ToArray();
            Check(window.Lines.Length == beforeDonutLineCount &&
                window.EntityCount == beforeDonutEntityCount + 1 &&
                window.LastTransactionSequence == beforeDonutTransaction + 1 &&
                donut.PointCount > 8 && Math.Abs(donut.PolyWidth - 2) < 0.000001 && !donut.IsLine &&
                Math.Abs(donutXs.Min() - 306) < 0.1 && Math.Abs(donutXs.Max() - 314) < 0.1,
                $"DONUT 6/10 preserves mean radius 4 and native width 2 " +
                $"(lines={window.Lines.Length}/{beforeDonutLineCount}, " +
                $"entities={window.EntityCount}/{beforeDonutEntityCount + 1}, " +
                $"tx={window.LastTransactionSequence}/{beforeDonutTransaction + 1}, " +
                $"points={donut.PointCount}, polyWidth={donut.PolyWidth}, isLine={donut.IsLine}, " +
                $"x={donutXs.Min()}..{donutXs.Max()})");
            window.SelectAt(new ArcCadPoint(314, 250));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == donutId && tree.PropertyType.Text == "LWPOLYLINE",
                "DONUT ring is selectable as its native LWPOLYLINE");
            Check(donut.AnalyticLength is { } donutLength &&
                Math.Abs(donutLength - 25.132741228718345) < 0.000000001 &&
                Math.Abs(donut.VisibleLength - donutLength) > 0.001 &&
                tree.PropertyLength.Text == "25.13",
                "DONUT uses exact bulge-polyline length instead of render chords");
            window.ListSelectedEntity();
            Check((tree.StatusText.Text ?? string.Empty).Contains($"LWPOLYLINE #{donutId}", StringComparison.Ordinal),
                "DONUT LIST reports native type and ID");
            window.Undo();
            Check(window.EntityCount == beforeDonutEntityCount, "DONUT undo removes one entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == donutId)
                    .Vertices.Span.SequenceEqual(donut.Vertices.Span) &&
                window.Entities.ToArray().Single(entity => entity.EntityId == donutId).PolyWidth == donut.PolyWidth &&
                window.Entities.ToArray().Single(entity => entity.EntityId == donutId).AnalyticLength ==
                    donut.AnalyticLength,
                "DONUT redo restores exact ID, geometry, width and analytic length");

            EnterCommand(window, "DONUT 8");
            Check(window.ActiveDrawingTool == "Donut", "DONUT disk route active");
            window.MovePointer(new ArcCadPoint(390, 250));
            Check(tree.Viewport.PreviewVertices.Length == 96 &&
                window.LastTransactionSequence == beforeDonutTransaction + 1,
                "DONUT disk preview creates no transaction");
            window.AcceptPoint(new ArcCadPoint(390, 250));
            var diskId = window.LastCreatedEntityId;
            var disk = window.Entities.ToArray().Single(entity => entity.EntityId == diskId);
            var diskCoordinates = disk.Vertices.ToArray();
            var diskXs = diskCoordinates.Where((_, index) => (index & 1) == 0).ToArray();
            Check(window.Lines.Length == beforeDonutLineCount &&
                window.EntityCount == beforeDonutEntityCount + 2 &&
                window.LastTransactionSequence == beforeDonutTransaction + 2 &&
                Math.Abs(disk.PolyWidth - 4) < 0.000001 && !disk.IsLine &&
                Math.Abs(diskXs.Min() - 388) < 0.1 && Math.Abs(diskXs.Max() - 392) < 0.1,
                "DONUT disk preserves mean radius 2 and native width 4");

            var beforePointEntityCount = window.EntityCount;
            var beforePointPathCount = window.Entities.Length;
            var beforePointMarkerCount = window.Markers.Length;
            var beforePointLineCount = window.Lines.Length;
            var beforePointTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "POINT");
            Check(window.ActiveDrawingTool == "Point", "POINT command route active");
            window.MovePointer(new ArcCadPoint(520, 250));
            Check(tree.Viewport.PreviewVertices.Length == 0 &&
                tree.Viewport.HasCursor && window.EntityCount == beforePointEntityCount &&
                window.LastTransactionSequence == beforePointTransaction,
                "POINT hover is visible and consumes no entity or transaction");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None) &&
                !window.IsLineActive && window.EntityCount == beforePointEntityCount,
                "POINT cancel creates no entity or transaction");

            EnterCommand(window, "PO");
            Check(window.ActiveDrawingTool == "Point", "PO alias route active");
            ExpectThrows<ArgumentOutOfRangeException>(
                () => window.AcceptPoint(new ArcCadPoint(double.NaN, 250)),
                "POINT rejects a non-finite coordinate before native mutation");
            Check(window.EntityCount == beforePointEntityCount &&
                window.LastTransactionSequence == beforePointTransaction,
                "POINT invalid coordinate creates no entity or transaction");
            window.AcceptPoint(new ArcCadPoint(520, 250));
            Dispatcher.UIThread.RunJobs();
            var pointId = window.LastCreatedEntityId;
            var pointMarker = window.Markers.ToArray().Single(marker => marker.EntityId == pointId);
            Check(window.Entities.Length == beforePointPathCount &&
                window.Markers.Length == beforePointMarkerCount + 1 &&
                window.Lines.Length == beforePointLineCount &&
                window.EntityCount == beforePointEntityCount + 1 &&
                window.LastTransactionSequence == beforePointTransaction + 1 &&
                pointMarker == new CadMarker(pointId, 520, 250) &&
                tree.Viewport.Markers.Span.Contains(pointMarker),
                "POINT creates one native marker in one transaction without a fake path");
            var pointPixel = WorldPixel(tree, pointMarker.X, pointMarker.Y);
            CheckColorNear(CaptureFrame(window), pointPixel, Drawing, 5,
                "POINT neutral marker glyph");
            window.SelectAt(new ArcCadPoint(520, 250));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == pointId && window.SelectedLine is null &&
                tree.Viewport.SelectedEntityId == pointId && tree.PropertyType.Text == "POINT" &&
                tree.PropertyId.Text == pointId.ToString() &&
                tree.PropertyLength.Text == "X 520.00 · Y 250.00",
                "POINT native marker is selectable with type, ID and coordinates");
            CheckColorNear(CaptureFrame(window), pointPixel, Cyan, 2,
                "POINT selected marker highlight");
            window.ListSelectedEntity();
            Check((tree.StatusText.Text ?? string.Empty).Contains($"POINT #{pointId}", StringComparison.Ordinal),
                "POINT LIST reports native type and ID");
            window.Undo();
            Check(window.EntityCount == beforePointEntityCount &&
                window.Markers.ToArray().All(marker => marker.EntityId != pointId),
                "POINT undo removes exactly one marker");
            window.Redo();
            Check(window.Markers.ToArray().Single(marker => marker.EntityId == pointId) == pointMarker,
                "POINT redo restores exact ID and coordinates");

            var beforeCloudEntityCount = window.EntityCount;
            var beforeCloudPathCount = window.Entities.Length;
            var beforeCloudMarkerCount = window.Markers.Length;
            var beforeCloudLineCount = window.Lines.Length;
            var beforeCloudTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "REVCLOUD");
            Check(!window.IsLineActive &&
                (tree.StatusText.Text ?? string.Empty).Contains("Uso: REVCLOUD", StringComparison.Ordinal),
                "REVCLOUD without arc length reports usage without mutation");
            EnterCommand(window, "REVCLOUD 0");
            Check(!window.IsLineActive && window.EntityCount == beforeCloudEntityCount &&
                window.LastTransactionSequence == beforeCloudTransaction,
                "REVCLOUD rejects non-positive arc length without mutation");

            EnterCommand(window, "RC 10 INVALID");
            Check(!window.IsLineActive && window.EntityCount == beforeCloudEntityCount &&
                window.LastTransactionSequence == beforeCloudTransaction &&
                (tree.StatusText.Text ?? string.Empty).Contains("NORMAL o CALLIGRAPHY", StringComparison.Ordinal),
                "REVCLOUD rejects an unknown style without mutation");

            EnterCommand(window, "RC 1E-300 NORMAL");
            Check(window.ActiveDrawingTool == "RevisionCloud" &&
                window.ActiveRevisionCloudStyle == "NORMAL",
                "RC tiny arc length reaches the bounded native route");
            window.AcceptPoint(new ArcCadPoint(480, 80));
            ExpectThrows<ArcCadCommandException>(
                () => window.AcceptPoint(new ArcCadPoint(570, 150)),
                "REVCLOUD rejects a segment count above the native cap");
            Check(window.EntityCount == beforeCloudEntityCount &&
                window.LastTransactionSequence == beforeCloudTransaction,
                "REVCLOUD cap rejection is atomic");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "REVCLOUD cap rejection can cancel");

            EnterCommand(window, "RC 10 NORMAL");
            Check(window.ActiveDrawingTool == "RevisionCloud" &&
                window.ActiveRevisionCloudStyle == "NORMAL",
                "REVCLOUD alias and NORMAL style route active after rejection");
            window.AcceptPoint(new ArcCadPoint(480, 80));
            window.MovePointer(new ArcCadPoint(480, 80));
            ExpectThrows<ArgumentException>(
                () => window.AcceptPoint(new ArcCadPoint(480, 80)),
                "REVCLOUD rejects a degenerate rectangle before native mutation");
            Check(window.EntityCount == beforeCloudEntityCount &&
                window.LastTransactionSequence == beforeCloudTransaction,
                "REVCLOUD degenerate rectangle creates no entity or transaction");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "REVCLOUD degenerate cancel handled");

            EnterCommand(window, "REVCLOUD 10");
            window.AcceptPoint(new ArcCadPoint(480, 80));
            window.MovePointer(new ArcCadPoint(570, 150));
            Check(tree.Viewport.PreviewVertices.Length == 16 &&
                window.EntityCount == beforeCloudEntityCount &&
                window.LastTransactionSequence == beforeCloudTransaction,
                "REVCLOUD rectangular preview is four ephemeral segments");
            window.AcceptPoint(new ArcCadPoint(570, 150));
            var revisionCloudId = window.LastCreatedEntityId;
            var revisionCloud = window.Entities.ToArray()
                .Single(entity => entity.EntityId == revisionCloudId);
            var cloudCoordinates = revisionCloud.Vertices.ToArray();
            var cloudXs = cloudCoordinates.Where((_, index) => (index & 1) == 0).ToArray();
            var cloudYs = cloudCoordinates.Where((_, index) => (index & 1) != 0).ToArray();
            Check(window.EntityCount == beforeCloudEntityCount + 1 &&
                window.Entities.Length == beforeCloudPathCount + 1 &&
                window.Markers.Length == beforeCloudMarkerCount &&
                window.Lines.Length == beforeCloudLineCount &&
                window.LastTransactionSequence == beforeCloudTransaction + 1 &&
                revisionCloud.PointCount > 8 && !revisionCloud.IsLine &&
                cloudXs.Max() - cloudXs.Min() >= 90 && cloudYs.Max() - cloudYs.Min() >= 70,
                "REVCLOUD creates one curved closed native entity in one transaction");
            var cloudPick = new ArcCadPoint(cloudCoordinates[0], cloudCoordinates[1]);
            var cloudVisibleIndex = Enumerable.Range(0, cloudCoordinates.Length / 2)
                .First(index => cloudCoordinates[index * 2] > 550);
            var cloudVisiblePick = new ArcCadPoint(
                cloudCoordinates[cloudVisibleIndex * 2],
                cloudCoordinates[cloudVisibleIndex * 2 + 1]);
            window.SelectAt(cloudPick);
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == revisionCloudId && window.SelectedLine is null &&
                tree.PropertyType.Text == "LWPOLYLINE",
                "REVCLOUD is selectable as its native closed polyline");
            window.ListSelectedEntity();
            Check((tree.StatusText.Text ?? string.Empty).Contains(
                    $"LWPOLYLINE #{revisionCloudId}", StringComparison.Ordinal),
                "REVCLOUD LIST reports native type and ID");
            window.Undo();
            Check(window.EntityCount == beforeCloudEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != revisionCloudId),
                "REVCLOUD undo removes exactly one entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == revisionCloudId)
                    .Vertices.Span.SequenceEqual(revisionCloud.Vertices.Span),
                "REVCLOUD redo restores exact ID and render geometry");

            var beforeCalligraphyEntityCount = window.EntityCount;
            var beforeCalligraphyTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "REVCLOUD 10 CALLIGRAPHY");
            Check(window.ActiveDrawingTool == "RevisionCloud" &&
                window.ActiveRevisionCloudStyle == "CALLIGRAPHY",
                "REVCLOUD CALLIGRAPHY style route active");
            window.AcceptPoint(new ArcCadPoint(820, 420));
            window.MovePointer(new ArcCadPoint(910, 490));
            Check(tree.Viewport.PreviewVertices.Length == 16 &&
                window.EntityCount == beforeCalligraphyEntityCount,
                "REVCLOUD CALLIGRAPHY keeps an ephemeral contour preview");
            window.AcceptPoint(new ArcCadPoint(910, 490));
            var calligraphyCloudId = window.LastCreatedEntityId;
            var calligraphyCloud = window.Entities.ToArray()
                .Single(entity => entity.EntityId == calligraphyCloudId);
            var calligraphyCoordinates = calligraphyCloud.Vertices.ToArray();
            var calligraphyPick = new ArcCadPoint(calligraphyCoordinates[0], calligraphyCoordinates[1]);
            Check(window.EntityCount == beforeCalligraphyEntityCount + 1 &&
                window.LastTransactionSequence == beforeCalligraphyTransaction + 1 &&
                calligraphyCloud.PointCount > 8 && !calligraphyCloud.IsLine &&
                !NormalizedShape(calligraphyCloud.Vertices.Span)
                    .SequenceEqual(NormalizedShape(revisionCloud.Vertices.Span)),
                "REVCLOUD CALLIGRAPHY creates distinct curved geometry in one transaction");
            CheckColorNear(
                CaptureFrame(window),
                WorldPixel(tree, calligraphyPick.X, calligraphyPick.Y),
                Drawing,
                2,
                "REVCLOUD CALLIGRAPHY renders visible pixels");
            window.SelectAt(calligraphyPick);
            var calligraphyBounds = window.MeasureSelectedBounds();
            var calligraphyBox = ParseMeasuredBounds(calligraphyBounds);
            Check(window.SelectedEntityId == calligraphyCloudId &&
                tree.PropertyType.Text == "LWPOLYLINE" &&
                Math.Abs(calligraphyBox.MinX - 816.25) < 0.001 &&
                Math.Abs(calligraphyBox.MinY - 416.25) < 0.001 &&
                Math.Abs(calligraphyBox.MaxX - 913.75) < 0.001 &&
                Math.Abs(calligraphyBox.MaxY - 493.75) < 0.001,
                $"REVCLOUD CALLIGRAPHY supports hit and exact extents; actual={calligraphyBounds}");
            window.Undo();
            Check(window.Entities.ToArray().All(entity => entity.EntityId != calligraphyCloudId),
                "REVCLOUD CALLIGRAPHY undo removes one entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == calligraphyCloudId)
                    .Vertices.Span.SequenceEqual(calligraphyCloud.Vertices.Span),
                "REVCLOUD CALLIGRAPHY redo restores exact ID and geometry");

            window.SelectAt(new ArcCadPoint(520, 250));
            Check(window.SelectedEntityId == pointId, "REVCLOUD CONVERT negative selects an ineligible POINT");
            var beforeRejectedConvertEntities = window.EntityCount;
            var beforeRejectedConvertTransaction = window.LastTransactionSequence;
            ExpectThrows<ArcCadCommandException>(
                () => EnterCommand(window, "RC CONVERT 10 NORMAL"),
                "REVCLOUD CONVERT rejects a non-Polyline source");
            Check(window.EntityCount == beforeRejectedConvertEntities &&
                window.LastTransactionSequence == beforeRejectedConvertTransaction &&
                window.SelectedEntityId == pointId,
                "REVCLOUD CONVERT rejection is atomic and preserves selection");

            EnterCommand(window, "RECTANG");
            window.AcceptPoint(new ArcCadPoint(650, 420));
            window.AcceptPoint(new ArcCadPoint(740, 490));
            var convertSourceId = window.LastCreatedEntityId;
            var convertSource = window.Entities.ToArray().Single(entity => entity.EntityId == convertSourceId);
            window.SelectAt(new ArcCadPoint(650, 420));
            Check(window.SelectedEntityId == convertSourceId,
                "REVCLOUD CONVERT selects a closed Polyline source");
            var beforeConvertEntityCount = window.EntityCount;
            var beforeConvertTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "RC CONVERT 14 CALLIGRAPHY");
            var convertedCloudId = window.LastCreatedEntityId;
            var convertedCloud = window.Entities.ToArray().Single(entity => entity.EntityId == convertedCloudId);
            var convertedCoordinates = convertedCloud.Vertices.ToArray();
            var convertedPick = new ArcCadPoint(convertedCoordinates[0], convertedCoordinates[1]);
            Check(convertedCloudId != convertSourceId &&
                window.EntityCount == beforeConvertEntityCount &&
                window.LastTransactionSequence == beforeConvertTransaction + 1 &&
                window.Entities.ToArray().All(entity => entity.EntityId != convertSourceId) &&
                convertedCloud.PointCount > 8 && !convertedCloud.IsLine,
                "REVCLOUD CONVERT replaces one closed Polyline atomically with a new curved ID");
            window.SelectAt(convertedPick);
            var convertedBounds = window.MeasureSelectedBounds();
            var convertedBox = ParseMeasuredBounds(convertedBounds);
            Check(window.SelectedEntityId == convertedCloudId &&
                tree.PropertyType.Text == "LWPOLYLINE" &&
                Math.Abs(convertedBox.MinX - 644.75) < 0.001 &&
                Math.Abs(convertedBox.MinY - 414.375) < 0.001 &&
                Math.Abs(convertedBox.MaxX - 745.25) < 0.001 &&
                Math.Abs(convertedBox.MaxY - 495.625) < 0.001,
                $"REVCLOUD CONVERT result supports hit and exact extents; actual={convertedBounds}");
            window.Undo();
            Check(window.Entities.ToArray().Any(entity => entity.EntityId == convertSourceId) &&
                window.Entities.ToArray().All(entity => entity.EntityId != convertedCloudId),
                "REVCLOUD CONVERT undo restores the original ID");
            window.Redo();
            Check(window.Entities.ToArray().All(entity => entity.EntityId != convertSourceId) &&
                window.Entities.ToArray().Single(entity => entity.EntityId == convertedCloudId)
                    .Vertices.Span.SequenceEqual(convertedCloud.Vertices.Span),
                "REVCLOUD CONVERT redo restores the new ID and exact geometry");

            var beforeWipeoutEntityCount = window.EntityCount;
            var beforeWipeoutPathCount = window.Entities.Length;
            var beforeWipeoutMarkerCount = window.Markers.Length;
            var beforeWipeoutLineCount = window.Lines.Length;
            var beforeWipeoutTransaction = window.LastTransactionSequence!.Value;
            var maskTargetPixel = WorldPixel(tree, cloudPick.X, cloudPick.Y);
            var beforeWipeoutFrame = CaptureFrame(window);
            Check(ColorCount(beforeWipeoutFrame, maskTargetPixel, Drawing, 12) > 0,
                "WIPEOUT oracle starts on visible prior path");

            EnterCommand(window, "WIPEOUT");
            Check(window.ActiveDrawingTool == "Wipeout", "WIPEOUT command route active");
            window.AcceptPoint(new ArcCadPoint(440, 40));
            window.AcceptPoint(new ArcCadPoint(530, 40));
            Check(window.HandleKey(Key.Escape, KeyModifiers.None) &&
                window.EntityCount == beforeWipeoutEntityCount &&
                window.LastTransactionSequence == beforeWipeoutTransaction,
                "WIPEOUT two-point cancel creates no entity or transaction");

            EnterCommand(window, "WIPEOUT");
            window.AcceptPoint(new ArcCadPoint(440, 40));
            window.AcceptPoint(new ArcCadPoint(530, 40));
            window.MovePointer(new ArcCadPoint(530, 120));
            Check(tree.Viewport.PreviewVertices.Length == 12 &&
                window.EntityCount == beforeWipeoutEntityCount,
                "WIPEOUT three-point candidate preview is ephemeral and closed");
            window.AcceptPoint(new ArcCadPoint(530, 120));
            window.MovePointer(new ArcCadPoint(440, 120));
            Check(tree.Viewport.PreviewVertices.Length == 16 &&
                window.LastTransactionSequence == beforeWipeoutTransaction,
                "WIPEOUT four-point preview consumes no transaction");
            window.AcceptPoint(new ArcCadPoint(440, 120));
            Check(window.HandleKey(Key.Enter, KeyModifiers.None), "WIPEOUT Enter commit handled");
            Dispatcher.UIThread.RunJobs();
            var wipeoutId = window.LastCreatedEntityId;
            var wipeout = window.Entities.ToArray().Single(entity => entity.EntityId == wipeoutId);
            Check(window.EntityCount == beforeWipeoutEntityCount + 1 &&
                window.Entities.Length == beforeWipeoutPathCount + 1 &&
                window.Markers.Length == beforeWipeoutMarkerCount &&
                window.Lines.Length == beforeWipeoutLineCount &&
                window.LastTransactionSequence == beforeWipeoutTransaction + 1 &&
                wipeout.IsMask && !wipeout.IsLine && wipeout.PointCount == 5 &&
                wipeout.Vertices.Span[0] == wipeout.Vertices.Span[^2] &&
                wipeout.Vertices.Span[1] == wipeout.Vertices.Span[^1],
                "WIPEOUT creates one closed native mask in one transaction");
            var maskedFrame = CaptureFrame(window);
            var maskBackground = (tree.Viewport.Background as ISolidColorBrush)?.Color
                ?? Color.FromArgb(255, 0x11, 0x18, 0x20);
            Check(ColorCount(maskedFrame, maskTargetPixel, Drawing, 8) == 0 &&
                ColorCount(maskedFrame, maskTargetPixel, maskBackground, 2) > 0,
                "WIPEOUT fill hides the prior path with viewport background");
            var wipeoutCornerPixel = WorldPixel(tree, 440, 120);
            Check(ColorCount(maskedFrame, wipeoutCornerPixel, maskBackground, 4) < 81,
                "WIPEOUT antialiased neutral frame remains visible");

            window.SelectAt(new ArcCadPoint(440, 120));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == wipeoutId && window.SelectedLine is null &&
                tree.PropertyType.Text == "WIPEOUT",
                "WIPEOUT native mask is selectable and typed");
            CheckColorNear(CaptureFrame(window), wipeoutCornerPixel, Cyan, 4,
                "WIPEOUT selected frame highlight");
            window.ListSelectedEntity();
            Check((tree.StatusText.Text ?? string.Empty).Contains($"WIPEOUT #{wipeoutId}", StringComparison.Ordinal),
                "WIPEOUT LIST reports native type and ID");
            window.Undo();
            Dispatcher.UIThread.RunJobs();
            Check(window.EntityCount == beforeWipeoutEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != wipeoutId) &&
                ColorCount(CaptureFrame(window), maskTargetPixel, Drawing, 12) > 0,
                "WIPEOUT undo removes mask and reveals prior path");
            window.Redo();
            Dispatcher.UIThread.RunJobs();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == wipeoutId)
                    .Vertices.Span.SequenceEqual(wipeout.Vertices.Span) &&
                ColorCount(CaptureFrame(window), maskTargetPixel, Drawing, 8) == 0,
                "WIPEOUT redo restores exact mask and hides prior path");

            var beforeClosedPolylineEntityCount = window.EntityCount;
            var beforeClosedPolylinePathCount = window.Entities.Length;
            var beforeClosedPolylineMarkerCount = window.Markers.Length;
            var beforeClosedPolylineLineCount = window.Lines.Length;
            var beforeClosedPolylineTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "PL");
            window.AcceptPoint(new ArcCadPoint(580, 180));
            window.AcceptPoint(new ArcCadPoint(650, 180));
            EnterCommand(window, "C");
            Check(window.ActiveDrawingTool == "Polyline" &&
                window.EntityCount == beforeClosedPolylineEntityCount &&
                window.LastTransactionSequence == beforeClosedPolylineTransaction &&
                (tree.StatusText.Text ?? string.Empty).Contains("3 vértices", StringComparison.Ordinal),
                "PLINE CLOSE rejects fewer than three vertices without mutation and stays active");
            window.CancelLine();

            EnterCommand(window, "PLINE");
            Check(window.ActiveDrawingTool == "Polyline", "PLINE command route active");
            window.AcceptPoint(new ArcCadPoint(580, 180));
            window.AcceptPoint(new ArcCadPoint(650, 180));
            window.AcceptPoint(new ArcCadPoint(650, 240));
            window.AcceptPoint(new ArcCadPoint(580, 240));
            Check(tree.Viewport.PreviewVertices.Length == 12 &&
                window.EntityCount == beforeClosedPolylineEntityCount &&
                window.LastTransactionSequence == beforeClosedPolylineTransaction,
                "PLINE CLOSE four-vertex preview is ephemeral");
            EnterCommand(window, "CLOSE");
            var closedPolylineId = window.LastCreatedEntityId;
            var closedPolyline = window.Entities.ToArray()
                .Single(entity => entity.EntityId == closedPolylineId);
            Check(window.EntityCount == beforeClosedPolylineEntityCount + 1 &&
                window.Entities.Length == beforeClosedPolylinePathCount + 1 &&
                window.Markers.Length == beforeClosedPolylineMarkerCount &&
                window.Lines.Length == beforeClosedPolylineLineCount &&
                window.LastTransactionSequence == beforeClosedPolylineTransaction + 1 &&
                closedPolyline.PointCount == 5 && !closedPolyline.IsLine && !closedPolyline.IsMask &&
                closedPolyline.Vertices.Span[0] == closedPolyline.Vertices.Span[^2] &&
                closedPolyline.Vertices.Span[1] == closedPolyline.Vertices.Span[^1],
                "PLINE CLOSE creates one closed native polyline in one transaction");
            window.SelectAt(new ArcCadPoint(615, 180));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == closedPolylineId && window.SelectedLine is null &&
                tree.PropertyType.Text == "LWPOLYLINE",
                "PLINE CLOSE native entity is selectable and typed");
            var closedPolylineArea = window.MeasureSelectedArea();
            Check(closedPolylineArea.Contains("Area = 4200", StringComparison.Ordinal) &&
                closedPolylineArea.Contains("Perimeter = 260", StringComparison.Ordinal),
                "PLINE CLOSE AREA reports exact room area and perimeter");
            window.ListSelectedEntity();
            Check((tree.StatusText.Text ?? string.Empty).Contains("closed true", StringComparison.Ordinal),
                "PLINE CLOSE LIST reports native closed state");
            window.Undo();
            Check(window.EntityCount == beforeClosedPolylineEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != closedPolylineId),
                "PLINE CLOSE undo removes exactly one entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == closedPolylineId)
                    .Vertices.Span.SequenceEqual(closedPolyline.Vertices.Span),
                "PLINE CLOSE redo restores exact ID and geometry");
            EnterCommand(window, "C");
            Check(window.ActiveDrawingTool == "Circle", "idle C alias still starts CIRCLE");
            window.CancelLine();
            Check(window.LastTransactionSequence == beforeClosedPolylineTransaction + 1,
                "cancelled idle CIRCLE creates no transaction");

            var beforeExplodeRejectionEntityCount = window.EntityCount;
            var beforeExplodeRejectionLineCount = window.Lines.Length;
            var beforeExplodeRejectionTransaction = window.LastTransactionSequence!.Value;
            window.SelectAt(new ArcCadPoint(760, 280));
            Check(window.SelectedEntityId is null, "EXPLODE no-selection setup");
            EnterCommand(window, "EXPLODE");
            Check(window.IsBackendConnected && window.EntityCount == beforeExplodeRejectionEntityCount &&
                window.Lines.Length == beforeExplodeRejectionLineCount &&
                window.LastTransactionSequence == beforeExplodeRejectionTransaction &&
                (tree.StatusText.Text ?? string.Empty).Contains("Seleccione", StringComparison.Ordinal),
                "EXPLODE without selection is non-mutating");
            window.SelectAt(new ArcCadPoint(250, 74));
            Check(tree.PropertyType.Text == "LINE", "EXPLODE rejection setup selects LINE");
            EnterCommand(window, "X");
            Check(window.IsBackendConnected && window.EntityCount == beforeExplodeRejectionEntityCount &&
                window.Lines.Length == beforeExplodeRejectionLineCount &&
                window.LastTransactionSequence == beforeExplodeRejectionTransaction &&
                (tree.StatusText.Text ?? string.Empty).Contains("LWPOLYLINE", StringComparison.Ordinal),
                "EXPLODE rejects LINE before native mutation and keeps backend connected");

            EnterCommand(window, "PLINE");
            window.AcceptPoint(new ArcCadPoint(680, 80));
            window.AcceptPoint(new ArcCadPoint(740, 80));
            window.AcceptPoint(new ArcCadPoint(740, 140));
            window.AcceptPoint(new ArcCadPoint(680, 140));
            EnterCommand(window, "CLOSE");
            var explodeSourceId = window.LastCreatedEntityId;
            var explodeSource = window.Entities.ToArray()
                .Single(entity => entity.EntityId == explodeSourceId);
            var beforeExplodeEntityCount = window.EntityCount;
            var beforeExplodeLineCount = window.Lines.Length;
            var beforeExplodeTransaction = window.LastTransactionSequence!.Value;
            var beforeExplodeLineIds = window.Lines.ToArray()
                .Select(line => line.EntityId)
                .ToHashSet();
            window.SelectAt(new ArcCadPoint(710, 80));
            EnterCommand(window, "X");
            var explodedLines = window.Lines.ToArray()
                .Where(line => !beforeExplodeLineIds.Contains(line.EntityId))
                .OrderBy(line => line.EntityId)
                .ToArray();
            var explodedLineIds = explodedLines.Select(line => line.EntityId).ToHashSet();
            Check(window.EntityCount == beforeExplodeEntityCount + 3 &&
                window.Lines.Length == beforeExplodeLineCount + 4 &&
                window.LastTransactionSequence == beforeExplodeTransaction + 1 &&
                window.Entities.ToArray().All(entity => entity.EntityId != explodeSourceId) &&
                explodedLines.Length == 4 &&
                explodedLines.Any(line => LineMatches(line, 680, 80, 740, 80)) &&
                explodedLines.Any(line => LineMatches(line, 740, 80, 740, 140)) &&
                explodedLines.Any(line => LineMatches(line, 740, 140, 680, 140)) &&
                explodedLines.Any(line => LineMatches(line, 680, 140, 680, 80)),
                "EXPLODE/X replaces one closed PLINE with four exact native LINE in one transaction");
            var explodedTopLine = explodedLines.Single(line => LineMatches(line, 680, 80, 740, 80));
            window.SelectAt(new ArcCadPoint(710, 80));
            Check(window.SelectedEntityId == explodedTopLine.EntityId && tree.PropertyType.Text == "LINE",
                "EXPLODE piece is independently selectable");
            window.ListSelectedEntity();
            Check((tree.StatusText.Text ?? string.Empty).Contains(
                    $"LINE #{explodedTopLine.EntityId}", StringComparison.Ordinal),
                "EXPLODE piece LIST reports its native ID");
            window.Undo();
            Check(window.EntityCount == beforeExplodeEntityCount &&
                window.Lines.Length == beforeExplodeLineCount &&
                window.Entities.ToArray().Single(entity => entity.EntityId == explodeSourceId)
                    .Vertices.Span.SequenceEqual(explodeSource.Vertices.Span) &&
                window.Lines.ToArray().All(line => !explodedLineIds.Contains(line.EntityId)),
                "EXPLODE undo restores source ID and removes all pieces");
            window.Redo();
            Check(window.Entities.ToArray().All(entity => entity.EntityId != explodeSourceId) &&
                window.Lines.ToArray()
                    .Where(line => explodedLineIds.Contains(line.EntityId))
                    .OrderBy(line => line.EntityId)
                    .SequenceEqual(explodedLines),
                "EXPLODE redo restores exact piece IDs and geometry");

            var beforeFilletEntityCount = window.EntityCount;
            var beforeFilletLineCount = window.Lines.Length;
            var beforeFilletTransaction = window.LastTransactionSequence!.Value;
            window.SelectAt(new ArcCadPoint(900, 320));
            Check(window.SelectedEntityId is null, "FILLET no-selection setup");
            EnterCommand(window, "F 0");
            Check(!window.IsLineActive && window.EntityCount == beforeFilletEntityCount &&
                window.LastTransactionSequence == beforeFilletTransaction &&
                (tree.StatusText.Text ?? string.Empty).Contains("Parámetro inválido", StringComparison.Ordinal),
                "FILLET rejects zero radius before mutation");
            EnterCommand(window, "F 10");
            Check(window.IsBackendConnected && !window.IsLineActive &&
                window.EntityCount == beforeFilletEntityCount &&
                window.LastTransactionSequence == beforeFilletTransaction &&
                (tree.StatusText.Text ?? string.Empty).Contains("Seleccione una LINE", StringComparison.Ordinal),
                "FILLET without selection is non-mutating");

            var filletFirstId = CreateLine(
                window,
                new ArcCadPoint(800, 180),
                new ArcCadPoint(880, 180));
            var filletSecondId = CreateLine(
                window,
                new ArcCadPoint(880, 180),
                new ArcCadPoint(880, 260));
            var filletFirstSource = FindLine(window, filletFirstId);
            var filletSecondSource = FindLine(window, filletSecondId);
            window.SelectAt(new ArcCadPoint(840, 180));
            var filletTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "FILLET 10");
            Check(window.ActiveDrawingTool == "Fillet" &&
                window.LastTransactionSequence == filletTransaction,
                "FILLET starts from the selected first LINE without mutation");
            window.AcceptPoint(new ArcCadPoint(880, 220));
            var filletArcId = window.LastCreatedEntityId;
            var filletFirst = FindLine(window, filletFirstId);
            var filletSecond = FindLine(window, filletSecondId);
            var filletArc = window.Entities.ToArray()
                .Single(entity => entity.EntityId == filletArcId);
            Check(window.EntityCount == beforeFilletEntityCount + 3 &&
                window.Lines.Length == beforeFilletLineCount + 2 &&
                window.LastTransactionSequence == filletTransaction + 1 &&
                LineMatches(filletFirst, 800, 180, 870, 180) &&
                LineMatches(filletSecond, 880, 190, 880, 260) &&
                filletArcId != filletFirstId && filletArcId != filletSecondId &&
                filletArc.AnalyticLength is { } filletLength &&
                Math.Abs(filletLength - 15.707963267948966) < 0.000000001,
                "FILLET trims two preserved LINE IDs and creates one exact tangent ARC in one transaction");
            window.SelectAt(new ArcCadPoint(877.0710678118655, 182.92893218813452));
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == filletArcId && tree.PropertyType.Text == "ARC" &&
                tree.PropertyLength.Text == "15.71" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Radius = 10.0000", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Arc length = 15.7080", StringComparison.Ordinal),
                "FILLET ARC is selectable and agrees with native radius/length inquiry");
            Dispatcher.UIThread.RunJobs();
            var filletFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "native-shapes", 1, filletFrame);
            window.Undo();
            Check(window.EntityCount == beforeFilletEntityCount + 2 &&
                FindLine(window, filletFirstId) == filletFirstSource &&
                FindLine(window, filletSecondId) == filletSecondSource &&
                window.Entities.ToArray().All(entity => entity.EntityId != filletArcId),
                "FILLET undo restores both source LINE and removes the ARC");
            window.Redo();
            Check(FindLine(window, filletFirstId) == filletFirst &&
                FindLine(window, filletSecondId) == filletSecond &&
                window.Entities.ToArray().Single(entity => entity.EntityId == filletArcId)
                    .Vertices.Span.SequenceEqual(filletArc.Vertices.Span),
                "FILLET redo restores exact LINE/ARC IDs and geometry");

            var beforeTtrEntityCount = window.EntityCount;
            var beforeTtrLineCount = window.Lines.Length;
            var beforeTtrTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "CIRCLE TTR 0");
            Check(!window.IsLineActive && window.EntityCount == beforeTtrEntityCount &&
                window.LastTransactionSequence == beforeTtrTransaction &&
                (tree.StatusText.Text ?? string.Empty).Contains("Uso: CIRCLE TTR", StringComparison.Ordinal),
                "CIRCLE TTR rejects zero radius before mutation");
            EnterCommand(window, "CIRCLE TTR 10");
            Check(window.ActiveDrawingTool == "CircleTtr" &&
                window.LastTransactionSequence == beforeTtrTransaction,
                "CIRCLE TTR starts without requiring preselection");
            ExpectThrows<InvalidOperationException>(
                () => window.AcceptPoint(new ArcCadPoint(960, 320)),
                "CIRCLE TTR requires a first LINE");
            Check(window.IsBackendConnected && window.EntityCount == beforeTtrEntityCount &&
                window.LastTransactionSequence == beforeTtrTransaction,
                "CIRCLE TTR invalid first pick keeps backend and scene intact");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None),
                "CIRCLE TTR invalid first pick can be cancelled");

            var ttrFirstId = CreateLine(
                window,
                new ArcCadPoint(960, 180),
                new ArcCadPoint(1040, 180));
            var ttrSecondId = CreateLine(
                window,
                new ArcCadPoint(1040, 180),
                new ArcCadPoint(1040, 260));
            var ttrFirstSource = FindLine(window, ttrFirstId);
            var ttrSecondSource = FindLine(window, ttrSecondId);
            var ttrTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "C TTR 10");
            window.AcceptPoint(new ArcCadPoint(1000, 180));
            Check(window.ActiveDrawingTool == "CircleTtr" && window.SelectedEntityId == ttrFirstId &&
                window.LastTransactionSequence == ttrTransaction,
                "C TTR alias preserves the first raw pick and awaits a second LINE");
            ExpectThrows<InvalidOperationException>(
                () => window.AcceptPoint(new ArcCadPoint(1020, 180)),
                "CIRCLE TTR rejects selecting the same LINE twice");
            Check(window.IsBackendConnected && window.EntityCount == beforeTtrEntityCount + 2 &&
                window.LastTransactionSequence == ttrTransaction && window.ActiveDrawingTool == "CircleTtr",
                "CIRCLE TTR same-LINE error is retryable and non-mutating");
            window.AcceptPoint(new ArcCadPoint(1040, 220));
            var ttrCircleId = window.LastCreatedEntityId;
            var ttrCircle = window.Entities.ToArray().Single(entity => entity.EntityId == ttrCircleId);
            var ttrCoordinates = ttrCircle.Vertices.ToArray();
            var ttrXs = ttrCoordinates.Where((_, index) => index % 2 == 0).ToArray();
            var ttrYs = ttrCoordinates.Where((_, index) => index % 2 != 0).ToArray();
            Check(window.EntityCount == beforeTtrEntityCount + 3 &&
                window.Lines.Length == beforeTtrLineCount + 2 &&
                window.LastTransactionSequence == ttrTransaction + 1 &&
                FindLine(window, ttrFirstId) == ttrFirstSource &&
                FindLine(window, ttrSecondId) == ttrSecondSource &&
                ttrCircleId != ttrFirstId && ttrCircleId != ttrSecondId &&
                Math.Abs(ttrXs.Min() - 1020) < 0.11 && Math.Abs(ttrXs.Max() - 1040) < 0.11 &&
                Math.Abs(ttrYs.Min() - 180) < 0.11 && Math.Abs(ttrYs.Max() - 200) < 0.11 &&
                ttrCircle.AnalyticLength is { } ttrLength &&
                Math.Abs(ttrLength - 62.83185307179586) < 0.000000001,
                $"CIRCLE TTR preserves both LINE and creates the selected center (1030,190), R10 in one transaction " +
                $"(entities={window.EntityCount}/{beforeTtrEntityCount + 3}, lines={window.Lines.Length}/{beforeTtrLineCount + 2}, " +
                $"tx={window.LastTransactionSequence}/{ttrTransaction + 1}, sources={FindLine(window, ttrFirstId) == ttrFirstSource}/" +
                $"{FindLine(window, ttrSecondId) == ttrSecondSource}, bounds={ttrXs.Min():G17},{ttrYs.Min():G17}-" +
                $"{ttrXs.Max():G17},{ttrYs.Max():G17}, analytic={ttrCircle.AnalyticLength:G17})");
            var ttrPick = new ArcCadPoint(1037.0710678118655, 197.07106781186548);
            window.SelectAt(ttrPick);
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == ttrCircleId && tree.PropertyType.Text == "CIRCLE" &&
                tree.PropertyLength.Text == "62.83" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Radius = 10", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Diameter = 20", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Circumference = 62.8319", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Area = 314.1593", StringComparison.Ordinal),
                "CIRCLE TTR is selectable and agrees with native analytic inquiry");
            Dispatcher.UIThread.RunJobs();
            var ttrFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "native-shapes", 2, ttrFrame);
            window.Undo();
            Check(window.EntityCount == beforeTtrEntityCount + 2 &&
                FindLine(window, ttrFirstId) == ttrFirstSource &&
                FindLine(window, ttrSecondId) == ttrSecondSource &&
                window.Entities.ToArray().All(entity => entity.EntityId != ttrCircleId),
                "CIRCLE TTR undo removes only the circle");
            window.Redo();
            Check(FindLine(window, ttrFirstId) == ttrFirstSource &&
                FindLine(window, ttrSecondId) == ttrSecondSource &&
                window.Entities.ToArray().Single(entity => entity.EntityId == ttrCircleId)
                    .Vertices.Span.SequenceEqual(ttrCircle.Vertices.Span),
                "CIRCLE TTR redo restores exact circle ID and geometry");

            var beforeCircleModesEntityCount = window.EntityCount;
            var beforeCircleModesTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "CIRCLE 2P");
            window.AcceptPoint(new ArcCadPoint(1080, 180));
            ExpectThrows<ArgumentException>(
                () => window.AcceptPoint(new ArcCadPoint(1080, 180)),
                "CIRCLE 2P rejects coincident diameter endpoints");
            Check(window.IsBackendConnected && window.EntityCount == beforeCircleModesEntityCount &&
                window.LastTransactionSequence == beforeCircleModesTransaction &&
                window.ActiveDrawingTool == "CircleTwoPoint",
                "CIRCLE 2P degenerate input is retryable and non-mutating");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None),
                "CIRCLE 2P degenerate input can be cancelled");

            EnterCommand(window, "C 2P");
            window.AcceptPoint(new ArcCadPoint(1080, 180));
            window.AcceptPoint(new ArcCadPoint(1120, 180));
            var circleTwoPointId = window.LastCreatedEntityId;
            var circleTwoPoint = window.Entities.ToArray()
                .Single(entity => entity.EntityId == circleTwoPointId);
            var circleTwoPointCoordinates = circleTwoPoint.Vertices.ToArray();
            var circleTwoPointXs = circleTwoPointCoordinates.Where((_, index) => index % 2 == 0).ToArray();
            var circleTwoPointYs = circleTwoPointCoordinates.Where((_, index) => index % 2 != 0).ToArray();
            Check(window.EntityCount == beforeCircleModesEntityCount + 1 &&
                window.LastTransactionSequence == beforeCircleModesTransaction + 1 &&
                Math.Abs(circleTwoPointXs.Min() - 1080) < 0.21 &&
                Math.Abs(circleTwoPointXs.Max() - 1120) < 0.21 &&
                Math.Abs(circleTwoPointYs.Min() - 160) < 0.21 &&
                Math.Abs(circleTwoPointYs.Max() - 200) < 0.21 &&
                circleTwoPoint.AnalyticLength is { } circleTwoPointLength &&
                Math.Abs(circleTwoPointLength - 125.66370614359172) < 0.000000001,
                "CIRCLE 2P creates center (1100,180), R20 and one exact native transaction");
            var circleTwoPointPick = new ArcCadPoint(1114.142135623731, 194.14213562373095);
            window.SelectAt(circleTwoPointPick);
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == circleTwoPointId && tree.PropertyType.Text == "CIRCLE" &&
                tree.PropertyLength.Text == "125.66" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Radius = 20", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Circumference = 125.6637", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Area = 1256.6371", StringComparison.Ordinal),
                "CIRCLE 2P is selectable and agrees with native analytic inquiry");
            window.Undo();
            Check(window.EntityCount == beforeCircleModesEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != circleTwoPointId),
                "CIRCLE 2P undo removes one circle");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == circleTwoPointId)
                    .Vertices.Span.SequenceEqual(circleTwoPoint.Vertices.Span),
                "CIRCLE 2P redo restores exact ID and geometry");

            var beforeThreePointEntityCount = window.EntityCount;
            var beforeThreePointTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "CIRCLE 3P");
            window.AcceptPoint(new ArcCadPoint(1150, 260));
            window.AcceptPoint(new ArcCadPoint(1170, 260));
            ExpectThrows<ArcCadCommandException>(
                () => window.AcceptPoint(new ArcCadPoint(1190, 260)),
                "CIRCLE 3P rejects collinear points in the native kernel");
            Check(window.IsBackendConnected && window.EntityCount == beforeThreePointEntityCount &&
                window.LastTransactionSequence == beforeThreePointTransaction &&
                window.ActiveDrawingTool == "CircleThreePoint",
                "CIRCLE 3P collinear error is retryable and non-mutating");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None),
                "CIRCLE 3P collinear input can be cancelled");

            EnterCommand(window, "C 3P");
            window.AcceptPoint(new ArcCadPoint(1150, 180));
            window.AcceptPoint(new ArcCadPoint(1170, 200));
            window.AcceptPoint(new ArcCadPoint(1190, 180));
            var circleThreePointId = window.LastCreatedEntityId;
            var circleThreePoint = window.Entities.ToArray()
                .Single(entity => entity.EntityId == circleThreePointId);
            var circleThreePointCoordinates = circleThreePoint.Vertices.ToArray();
            var circleThreePointXs = circleThreePointCoordinates.Where((_, index) => index % 2 == 0).ToArray();
            var circleThreePointYs = circleThreePointCoordinates.Where((_, index) => index % 2 != 0).ToArray();
            Check(window.EntityCount == beforeThreePointEntityCount + 1 &&
                window.LastTransactionSequence == beforeThreePointTransaction + 1 &&
                Math.Abs(circleThreePointXs.Min() - 1150) < 0.21 &&
                Math.Abs(circleThreePointXs.Max() - 1190) < 0.21 &&
                Math.Abs(circleThreePointYs.Min() - 160) < 0.21 &&
                Math.Abs(circleThreePointYs.Max() - 200) < 0.21 &&
                circleThreePoint.AnalyticLength is { } circleThreePointLength &&
                Math.Abs(circleThreePointLength - 125.66370614359172) < 0.000000001,
                "CIRCLE 3P creates circumcenter (1170,180), R20 and one exact native transaction");
            var circleThreePointPick = new ArcCadPoint(1184.142135623731, 194.14213562373095);
            window.SelectAt(circleThreePointPick);
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == circleThreePointId && tree.PropertyType.Text == "CIRCLE" &&
                tree.PropertyLength.Text == "125.66" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Radius = 20", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Diameter = 40", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Circumference = 125.6637", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Area = 1256.6371", StringComparison.Ordinal),
                "CIRCLE 3P is selectable and agrees with native analytic inquiry");
            Dispatcher.UIThread.RunJobs();
            var circleModesFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "native-shapes", 3, circleModesFrame);
            window.Undo();
            Check(window.EntityCount == beforeThreePointEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != circleThreePointId),
                "CIRCLE 3P undo removes one circle");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == circleThreePointId)
                    .Vertices.Span.SequenceEqual(circleThreePoint.Vertices.Span),
                "CIRCLE 3P redo restores exact ID and geometry");

            var beforeArcCseEntityCount = window.EntityCount;
            var beforeArcCseLineCount = window.Lines.Length;
            var beforeArcCseTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "ARC CSE");
            window.AcceptPoint(new ArcCadPoint(1060, 300));
            window.AcceptPoint(new ArcCadPoint(1060, 300));
            ExpectThrows<ArgumentException>(
                () => window.AcceptPoint(new ArcCadPoint(1080, 300)),
                "ARC CSE rejects coincident center and start");
            Check(window.IsBackendConnected && window.EntityCount == beforeArcCseEntityCount &&
                window.LastTransactionSequence == beforeArcCseTransaction &&
                window.ActiveDrawingTool == "ArcCenterStartEnd",
                "ARC CSE zero-radius error is retryable and non-mutating");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None),
                "ARC CSE zero-radius input can be cancelled");

            EnterCommand(window, "A CSE");
            window.AcceptPoint(new ArcCadPoint(1060, 300));
            window.AcceptPoint(new ArcCadPoint(1080, 300));
            window.AcceptPoint(new ArcCadPoint(1060, 320));
            var arcCseId = window.LastCreatedEntityId;
            var arcCse = window.Entities.ToArray().Single(entity => entity.EntityId == arcCseId);
            var arcCseCoordinates = arcCse.Vertices.Span;
            Check(window.EntityCount == beforeArcCseEntityCount + 1 &&
                window.Lines.Length == beforeArcCseLineCount &&
                window.LastTransactionSequence == beforeArcCseTransaction + 1 &&
                arcCse.PointCount > 2 &&
                Math.Abs(arcCseCoordinates[0] - 1080) < 0.001 &&
                Math.Abs(arcCseCoordinates[1] - 300) < 0.001 &&
                Math.Abs(arcCseCoordinates[^2] - 1060) < 0.001 &&
                Math.Abs(arcCseCoordinates[^1] - 320) < 0.001 &&
                arcCse.AnalyticLength is { } arcCseLength &&
                Math.Abs(arcCseLength - 31.41592653589793) < 0.000000001,
                "ARC CSE creates one exact CCW quarter arc in one native transaction");
            var arcCsePick = new ArcCadPoint(1074.142135623731, 314.14213562373095);
            window.SelectAt(arcCsePick);
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == arcCseId && tree.PropertyType.Text == "ARC" &&
                tree.PropertyLength.Text == "31.42" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Radius = 20", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Diameter = 40", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Arc length = 31.4159", StringComparison.Ordinal),
                "ARC CSE is selectable and agrees with native radius/length inquiry");
            Dispatcher.UIThread.RunJobs();
            var arcCseFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "native-shapes", 4, arcCseFrame);
            window.Undo();
            Check(window.EntityCount == beforeArcCseEntityCount &&
                window.Entities.ToArray().All(entity => entity.EntityId != arcCseId),
                "ARC CSE undo removes one arc");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == arcCseId)
                    .Vertices.Span.SequenceEqual(arcCse.Vertices.Span),
                "ARC CSE redo restores exact ID and geometry");

            Click(tree.OsnapStatus);
            Check(window.IsObjectSnapEnabled, "house demo object snap restored");

            var initialZoom = tree.Viewport.Zoom;
            Click(tree.ZoomInButton);
            Check(tree.Viewport.Zoom > initialZoom, "zoom-in visible route");
            window.ZoomOut();
            Check(Math.Abs(tree.Viewport.Zoom - initialZoom) < 0.000001, "zoom-out visible camera action");
            Click(tree.PanButton);
            Check(window.IsPanMode && tree.PanButton.Classes.Contains("active"), "pan mode route on");
            tree.Viewport.PanBy(new Vector(34, -18));
            Check(tree.Viewport.Pan == new Vector(34, -18), "pan changes visible camera offset");
            Click(tree.PanButton);
            Check(!window.IsPanMode && !tree.PanButton.Classes.Contains("active"), "pan mode route off");
            Click(tree.ResetViewButton);
            Check(tree.Viewport.Zoom == 1 && tree.Viewport.Pan == default, "reset view route");
            Click(tree.FitViewButton);
            Check(double.IsFinite(tree.Viewport.Zoom) && tree.Viewport.Zoom > 0 &&
                (Math.Abs(tree.Viewport.Zoom - 1) > 0.000001 || tree.Viewport.Pan != default),
                "fit drawing route");

            Click(tree.OsnapStatus);
            Check(!window.IsObjectSnapEnabled, "construction geometry exact test disables OSNAP");
            var beforeConstructionEntityCount = window.EntityCount;
            var beforeConstructionLineCount = window.Lines.Length;
            var beforeConstructionTransaction = window.LastTransactionSequence!.Value;
            EnterCommand(window, "XLINE A nope");
            Check(!window.IsLineActive && window.EntityCount == beforeConstructionEntityCount &&
                (tree.StatusText.Text ?? string.Empty).Contains("Uso: XLINE", StringComparison.Ordinal),
                "XLINE invalid angle reports usage without mutation");

            EnterCommand(window, "XL");
            Check(window.ActiveDrawingTool == "Xline", "XL alias route active");
            window.AcceptPoint(new ArcCadPoint(60, 340));
            window.MovePointer(new ArcCadPoint(60, 340));
            ExpectThrows<ArgumentException>(
                () => window.AcceptPoint(new ArcCadPoint(60, 340)),
                "XLINE rejects zero direction before native mutation");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None) && !window.IsLineActive &&
                window.EntityCount == beforeConstructionEntityCount &&
                window.LastTransactionSequence == beforeConstructionTransaction,
                "XLINE degenerate cancel creates no entity or transaction");

            EnterCommand(window, "XLINE");
            window.AcceptPoint(new ArcCadPoint(60, 340));
            window.MovePointer(new ArcCadPoint(100, 300));
            Check(tree.Viewport.PreviewVertices.Length == 4 &&
                window.EntityCount == beforeConstructionEntityCount,
                "XLINE two-point preview is ephemeral");
            window.AcceptPoint(new ArcCadPoint(100, 300));
            var xlineId = window.LastCreatedEntityId;
            var xline = window.Entities.ToArray().Single(entity => entity.EntityId == xlineId);
            var xlineCoordinates = xline.Vertices.ToArray();
            var xlineDx = xlineCoordinates[^2] - xlineCoordinates[0];
            var xlineDy = xlineCoordinates[^1] - xlineCoordinates[1];
            Check(window.Lines.Length == beforeConstructionLineCount &&
                window.EntityCount == beforeConstructionEntityCount + 1 &&
                window.LastTransactionSequence == beforeConstructionTransaction + 1 &&
                xline.PointCount >= 2 && Math.Abs(Math.Abs(xlineDx) - Math.Abs(xlineDy)) < 0.001,
                $"XLINE points creates one infinite diagonal entity in one transaction " +
                $"(lines={window.Lines.Length}/{beforeConstructionLineCount}, " +
                $"entities={window.EntityCount}/{beforeConstructionEntityCount + 1}, " +
                $"tx={window.LastTransactionSequence}/{beforeConstructionTransaction + 1}, " +
                $"points={xline.PointCount}, dx={xlineDx}, dy={xlineDy})");
            window.SelectAt(new ArcCadPoint(60, 340));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == xlineId && tree.PropertyType.Text == "XLINE",
                "XLINE points is selectable and typed");
            window.ListSelectedEntity();
            Check((tree.StatusText.Text ?? string.Empty).Contains($"XLINE #{xlineId}", StringComparison.Ordinal),
                "XLINE LIST reports native type and ID");
            window.Undo();
            Check(window.EntityCount == beforeConstructionEntityCount,
                "XLINE undo removes one entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == xlineId)
                    .Vertices.Span.SequenceEqual(xline.Vertices.Span),
                "XLINE redo restores exact ID and geometry");

            EnterCommand(window, "XLINE H");
            Check(window.ActiveDrawingTool == "XlineHorizontal", "XLINE H route active");
            window.AcceptPoint(new ArcCadPoint(80, 330));
            var horizontalXlineId = window.LastCreatedEntityId;
            var horizontalXline = window.Entities.ToArray().Single(entity => entity.EntityId == horizontalXlineId);
            var horizontalCoordinates = horizontalXline.Vertices.ToArray();
            Check(window.LastTransactionSequence == beforeConstructionTransaction + 2 &&
                horizontalCoordinates.Where((_, index) => (index & 1) != 0)
                    .All(y => Math.Abs(y - 330) < 0.001),
                "XLINE H creates one exact horizontal entity");

            EnterCommand(window, "XL VER");
            Check(window.ActiveDrawingTool == "XlineVertical", "XL VER route active");
            window.AcceptPoint(new ArcCadPoint(450, 100));
            var verticalXlineId = window.LastCreatedEntityId;
            var verticalXline = window.Entities.ToArray().Single(entity => entity.EntityId == verticalXlineId);
            var verticalCoordinates = verticalXline.Vertices.ToArray();
            Check(window.LastTransactionSequence == beforeConstructionTransaction + 3 &&
                verticalCoordinates.Where((_, index) => (index & 1) == 0)
                    .All(x => Math.Abs(x - 450) < 0.001),
                "XLINE V creates one exact vertical entity");

            EnterCommand(window, "XLINE ANG 30");
            Check(window.ActiveDrawingTool == "XlineAngle", "XLINE ANG route active");
            window.AcceptPoint(new ArcCadPoint(400, 310));
            var angledXlineId = window.LastCreatedEntityId;
            var angledXline = window.Entities.ToArray().Single(entity => entity.EntityId == angledXlineId);
            var angledCoordinates = angledXline.Vertices.ToArray();
            var angledDx = angledCoordinates[^2] - angledCoordinates[0];
            var angledDy = angledCoordinates[^1] - angledCoordinates[1];
            Check(window.LastTransactionSequence == beforeConstructionTransaction + 4 &&
                Math.Abs(Math.Abs(angledDy / angledDx) - Math.Tan(Math.PI / 6)) < 0.001,
                "XLINE ANG converts degrees and creates the exact direction");

            EnterCommand(window, "RAY");
            Check(window.ActiveDrawingTool == "Ray", "RAY route active");
            window.AcceptPoint(new ArcCadPoint(380, 320));
            window.MovePointer(new ArcCadPoint(430, 340));
            Check(tree.Viewport.PreviewVertices.Length == 4 &&
                window.LastTransactionSequence == beforeConstructionTransaction + 4,
                "RAY preview is ephemeral");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None) &&
                window.EntityCount == beforeConstructionEntityCount + 4,
                "RAY cancel creates no entity or transaction");
            EnterCommand(window, "RAY");
            window.AcceptPoint(new ArcCadPoint(380, 320));
            window.AcceptPoint(new ArcCadPoint(430, 340));
            var rayId = window.LastCreatedEntityId;
            var ray = window.Entities.ToArray().Single(entity => entity.EntityId == rayId);
            var rayCoordinates = ray.Vertices.ToArray();
            Check(window.Lines.Length == beforeConstructionLineCount &&
                tree.Viewport.Lines.Length == beforeConstructionLineCount &&
                window.EntityCount == beforeConstructionEntityCount + 5 &&
                window.LastTransactionSequence == beforeConstructionTransaction + 5 &&
                ray.PointCount >= 2 && Math.Abs(rayCoordinates[0] - 380) < 0.001 &&
                Math.Abs(rayCoordinates[1] - 320) < 0.001 &&
                (rayCoordinates[^2] - rayCoordinates[0]) * 50 +
                (rayCoordinates[^1] - rayCoordinates[1]) * 20 > 0,
                "RAY creates one directed native entity in one transaction");
            window.SelectAt(new ArcCadPoint(380, 320));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == rayId && tree.PropertyType.Text == "RAY",
                "RAY is selectable and typed");
            window.ListSelectedEntity();
            Check((tree.StatusText.Text ?? string.Empty).Contains($"RAY #{rayId}", StringComparison.Ordinal),
                "RAY LIST reports native type and ID");
            window.Undo();
            Check(window.EntityCount == beforeConstructionEntityCount + 4,
                "RAY undo removes one entity");
            window.Redo();
            Check(window.Entities.ToArray().Single(entity => entity.EntityId == rayId)
                    .Vertices.Span.SequenceEqual(ray.Vertices.Span),
                "RAY redo restores exact ID and geometry");
            Click(tree.OsnapStatus);
            Check(window.IsObjectSnapEnabled, "construction geometry restores OSNAP");

            window.SelectAt(new ArcCadPoint(710, 80));
            Check(window.SelectedEntityId == explodedTopLine.EntityId && tree.PropertyType.Text == "LINE",
                "EXPLODE independent wall selected for visible capture");
            window.SelectAt(new ArcCadPoint(350, 235));
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == circleId && tree.PropertyType.Text == "CIRCLE" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Radius = 20", StringComparison.Ordinal),
                "MEASUREGEOM Radius CIRCLE selected for visible capture");
            Dispatcher.UIThread.RunJobs();
            var beforeDimensionsFrame = CaptureFrame(window);
            Click(tree.DimensionButton);
            Dispatcher.UIThread.RunJobs();
            Check(tree.Viewport.ShowDimensions && tree.DimensionButton.Classes.Contains("active") &&
                tree.AnnotationStatus.Classes.Contains("active"), "dimension overlay route");
            var dimensionsFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "native-shapes", 5, dimensionsFrame);
            CheckDiffExistsInside(
                beforeDimensionsFrame.Pixels,
                dimensionsFrame.Pixels,
                dimensionsFrame.PixelSize,
                viewportPixels,
                "dimension labels visible");
            Click(tree.LineweightStatus);
            Dispatcher.UIThread.RunJobs();
            Check(tree.Viewport.UseHeavyLineweight && tree.LineweightStatus.Classes.Contains("active"),
                "lineweight overlay route");
            var houseFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "native-shapes", 6, houseFrame);
            CheckDiffExistsInside(
                dimensionsFrame.Pixels,
                houseFrame.Pixels,
                houseFrame.PixelSize,
                viewportPixels,
                "lineweight visible");
            window.SelectAt(ellipseArcPick);
            EnterCommand(window, "MEA BOUNDS");
            Check(window.SelectedEntityId == ellipseArcId && tree.PropertyType.Text == "ELLIPSE" &&
                tree.PropertyLength.Text == "48.44" &&
                string.Equals(tree.StatusText.Text, ellipseArcBounds, StringComparison.Ordinal),
                "ELLIPSE ARC capture state aligns Properties and native bounds");
            Dispatcher.UIThread.RunJobs();
            var ellipseArcFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "native-shapes", 7, ellipseArcFrame);
            capture = ellipseArcFrame.Png;
            var savedEntityCount = window.EntityCount;
            var savedLineCount = window.Lines.Length;
            var savedMarkerCount = window.Markers.Length;
            window.SaveToPath(featurePath);
            Check(File.Exists(featurePath) && !window.IsDirty, "house demo saved");
            window.NewDocument();
            Check(window.EntityCount == 0 && tree.Viewport.Entities.Length == 0 &&
                window.Markers.Length == 0 && tree.Viewport.Markers.Length == 0,
                "new document clears native paths and markers from viewport");
            var featureWarnings = window.OpenFromPath(featurePath);
            Check(featureWarnings.Count == 0 && window.EntityCount == savedEntityCount &&
                window.Lines.Length == savedLineCount && window.Markers.Length == savedMarkerCount,
                "house demo reopens native entity count without warnings");
            Check(window.LastTransactionSequence is null && !window.CanUndo && !window.CanRedo,
                "house demo reopen clears transaction history before queries");
            var reopenedCircle = window.Entities.ToArray().Single(entity => entity.EntityId == circleId);
            var reopenedArc = window.Entities.ToArray().Single(entity => entity.EntityId == arcId);
            var reopenedEllipse = window.Entities.ToArray().Single(entity => entity.EntityId == ellipseId);
            var reopenedRectangle = window.Entities.ToArray().Single(entity => entity.EntityId == rectangleId);
            var reopenedPolygon = window.Entities.ToArray().Single(entity => entity.EntityId == polygonId);
            var reopenedPolyline = window.Entities.ToArray().Single(entity => entity.EntityId == polylineId);
            var reopenedSpline = window.Entities.ToArray().Single(entity => entity.EntityId == splineId);
            var reopenedDonut = window.Entities.ToArray().Single(entity => entity.EntityId == donutId);
            var reopenedDisk = window.Entities.ToArray().Single(entity => entity.EntityId == diskId);
            var reopenedPoint = window.Markers.ToArray().Single(marker => marker.EntityId == pointId);
            var reopenedRevisionCloud = window.Entities.ToArray()
                .Single(entity => entity.EntityId == revisionCloudId);
            var reopenedCalligraphyCloud = window.Entities.ToArray()
                .Single(entity => entity.EntityId == calligraphyCloudId);
            var reopenedConvertedCloud = window.Entities.ToArray()
                .Single(entity => entity.EntityId == convertedCloudId);
            var reopenedWipeout = window.Entities.ToArray()
                .Single(entity => entity.EntityId == wipeoutId);
            var reopenedClosedPolyline = window.Entities.ToArray()
                .Single(entity => entity.EntityId == closedPolylineId);
            var reopenedXline = window.Entities.ToArray().Single(entity => entity.EntityId == xlineId);
            var reopenedHorizontalXline = window.Entities.ToArray().Single(entity => entity.EntityId == horizontalXlineId);
            var reopenedVerticalXline = window.Entities.ToArray().Single(entity => entity.EntityId == verticalXlineId);
            var reopenedAngledXline = window.Entities.ToArray().Single(entity => entity.EntityId == angledXlineId);
            var reopenedRay = window.Entities.ToArray().Single(entity => entity.EntityId == rayId);
            var reopenedFilletArc = window.Entities.ToArray()
                .Single(entity => entity.EntityId == filletArcId);
            var reopenedTtrCircle = window.Entities.ToArray()
                .Single(entity => entity.EntityId == ttrCircleId);
            var reopenedCircleTwoPoint = window.Entities.ToArray()
                .Single(entity => entity.EntityId == circleTwoPointId);
            var reopenedCircleThreePoint = window.Entities.ToArray()
                .Single(entity => entity.EntityId == circleThreePointId);
            var reopenedArcCse = window.Entities.ToArray()
                .Single(entity => entity.EntityId == arcCseId);
            var reopenedEllipseCenter = window.Entities.ToArray()
                .Single(entity => entity.EntityId == ellipseCenterId);
            var reopenedEllipseArc = window.Entities.ToArray()
                .Single(entity => entity.EntityId == ellipseArcId);
            Check(reopenedCircle.Vertices.Span.SequenceEqual(circle.Vertices.Span) &&
                reopenedArc.Vertices.Span.SequenceEqual(arc.Vertices.Span) &&
                reopenedEllipse.Vertices.Span.SequenceEqual(ellipse.Vertices.Span) &&
                reopenedRectangle.Vertices.Span.SequenceEqual(rectangle.Vertices.Span) &&
                reopenedPolygon.Vertices.Span.SequenceEqual(polygon.Vertices.Span) &&
                reopenedPolyline.Vertices.Span.SequenceEqual(polyline.Vertices.Span) &&
                reopenedSpline.Vertices.Span.SequenceEqual(spline.Vertices.Span) &&
                reopenedDonut.Vertices.Span.SequenceEqual(donut.Vertices.Span) &&
                reopenedDonut.PolyWidth == donut.PolyWidth &&
                reopenedDisk.Vertices.Span.SequenceEqual(disk.Vertices.Span) &&
                reopenedDisk.PolyWidth == disk.PolyWidth &&
                reopenedPoint == pointMarker &&
                reopenedRevisionCloud.Vertices.Span.SequenceEqual(revisionCloud.Vertices.Span) &&
                reopenedCalligraphyCloud.Vertices.Span.SequenceEqual(calligraphyCloud.Vertices.Span) &&
                reopenedConvertedCloud.Vertices.Span.SequenceEqual(convertedCloud.Vertices.Span) &&
                window.Entities.ToArray().All(entity => entity.EntityId != convertSourceId) &&
                reopenedWipeout.IsMask &&
                reopenedWipeout.Vertices.Span.SequenceEqual(wipeout.Vertices.Span) &&
                !reopenedClosedPolyline.IsLine && !reopenedClosedPolyline.IsMask &&
                reopenedClosedPolyline.Vertices.Span.SequenceEqual(closedPolyline.Vertices.Span) &&
                reopenedXline.Vertices.Span.SequenceEqual(xline.Vertices.Span) &&
                reopenedHorizontalXline.Vertices.Span.SequenceEqual(horizontalXline.Vertices.Span) &&
                reopenedVerticalXline.Vertices.Span.SequenceEqual(verticalXline.Vertices.Span) &&
                reopenedAngledXline.Vertices.Span.SequenceEqual(angledXline.Vertices.Span) &&
                reopenedRay.Vertices.Span.SequenceEqual(ray.Vertices.Span) &&
                FindLine(window, filletFirstId) == filletFirst &&
                FindLine(window, filletSecondId) == filletSecond &&
                reopenedFilletArc.Vertices.Span.SequenceEqual(filletArc.Vertices.Span) &&
                reopenedFilletArc.AnalyticLength == filletArc.AnalyticLength &&
                FindLine(window, ttrFirstId) == ttrFirstSource &&
                FindLine(window, ttrSecondId) == ttrSecondSource &&
                reopenedTtrCircle.Vertices.Span.SequenceEqual(ttrCircle.Vertices.Span) &&
                reopenedTtrCircle.AnalyticLength == ttrCircle.AnalyticLength &&
                reopenedCircleTwoPoint.Vertices.Span.SequenceEqual(circleTwoPoint.Vertices.Span) &&
                reopenedCircleTwoPoint.AnalyticLength == circleTwoPoint.AnalyticLength &&
                reopenedCircleThreePoint.Vertices.Span.SequenceEqual(circleThreePoint.Vertices.Span) &&
                reopenedCircleThreePoint.AnalyticLength == circleThreePoint.AnalyticLength &&
                reopenedArcCse.Vertices.Span.SequenceEqual(arcCse.Vertices.Span) &&
                reopenedArcCse.AnalyticLength == arcCse.AnalyticLength &&
                reopenedEllipseCenter.Vertices.Span.SequenceEqual(ellipseCenter.Vertices.Span) &&
                reopenedEllipseCenter.AnalyticLength == ellipseCenter.AnalyticLength &&
                reopenedEllipseArc.Vertices.Span.SequenceEqual(ellipseArc.Vertices.Span) &&
                reopenedEllipseArc.AnalyticLength == ellipseArc.AnalyticLength,
                "native paths, closed shapes and curves reopen with exact IDs and render geometry");
            window.SelectAt(new ArcCadPoint(100, 190));
            Check(window.SelectedEntityId == rectangleId && tree.PropertyType.Text == "LWPOLYLINE" &&
                window.MeasureSelectedArea().Contains("Area = 27000", StringComparison.Ordinal),
                "reopened RECTANG keeps type, ID and exact AREA");
            window.SelectAt(new ArcCadPoint(175, 200));
            Check(window.SelectedEntityId == polygonId && tree.PropertyType.Text == "LWPOLYLINE" &&
                window.MeasureSelectedArea().Contains("Area = 1623.7976", StringComparison.Ordinal),
                "reopened POLYGON keeps type, ID and exact AREA");
            window.SelectAt(new ArcCadPoint(350, 235));
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == circleId && tree.PropertyType.Text == "CIRCLE" &&
                tree.PropertyLength.Text == "125.66" && reopenedCircle.AnalyticLength == circle.AnalyticLength &&
                (tree.StatusText.Text ?? string.Empty).Contains(
                    "Circumference = 125.6637", StringComparison.Ordinal),
                "reopened CIRCLE keeps native ID, analytic property and MEASUREGEOM result");
            window.SelectAt(new ArcCadPoint(270, 120));
            EnterCommand(window, "MEASUREGEOM RADIUS");
            Check(window.SelectedEntityId == arcId && tree.PropertyType.Text == "ARC" &&
                tree.PropertyLength.Text == "62.83" && reopenedArc.AnalyticLength == arc.AnalyticLength &&
                (tree.StatusText.Text ?? string.Empty).Contains("Arc length = 62.8319", StringComparison.Ordinal),
                "reopened ARC keeps native ID, analytic property and MEASUREGEOM result");
            window.SelectAt(new ArcCadPoint(877.0710678118655, 182.92893218813452));
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == filletArcId && tree.PropertyType.Text == "ARC" &&
                tree.PropertyLength.Text == "15.71" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Radius = 10.0000", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Arc length = 15.7080", StringComparison.Ordinal),
                "reopened FILLET keeps tangent LINE/ARC IDs and exact radius/length");
            window.SelectAt(ttrPick);
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == ttrCircleId && tree.PropertyType.Text == "CIRCLE" &&
                tree.PropertyLength.Text == "62.83" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Circumference = 62.8319", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Area = 314.1593", StringComparison.Ordinal),
                "reopened CIRCLE TTR keeps source IDs, center, radius and analytic inquiry");
            window.SelectAt(circleTwoPointPick);
            Check(window.SelectedEntityId == circleTwoPointId && tree.PropertyType.Text == "CIRCLE" &&
                tree.PropertyLength.Text == "125.66",
                "reopened CIRCLE 2P keeps native ID, center, radius and analytic length");
            window.SelectAt(circleThreePointPick);
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == circleThreePointId && tree.PropertyType.Text == "CIRCLE" &&
                tree.PropertyLength.Text == "125.66" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Circumference = 125.6637", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Area = 1256.6371", StringComparison.Ordinal),
                "reopened CIRCLE 3P keeps native ID, circumcircle and analytic inquiry");
            window.SelectAt(arcCsePick);
            EnterCommand(window, "MEA RADIUS");
            Check(window.SelectedEntityId == arcCseId && tree.PropertyType.Text == "ARC" &&
                tree.PropertyLength.Text == "31.42" &&
                (tree.StatusText.Text ?? string.Empty).Contains("Radius = 20", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Arc length = 31.4159", StringComparison.Ordinal),
                "reopened ARC CSE keeps native ID, CCW sweep and analytic inquiry");
            window.SelectAt(new ArcCadPoint(390, 130));
            Check(window.SelectedEntityId == ellipseId && tree.PropertyType.Text == "ELLIPSE" &&
                tree.PropertyLength.Text == "290.65" &&
                reopenedEllipse.AnalyticLength == ellipse.AnalyticLength &&
                window.MeasureSelectedArea().Contains("Perimeter = 290.6534", StringComparison.Ordinal),
                "reopened ELLIPSE keeps native type and selection ID");
            window.SelectAt(ellipseCenterPick);
            var reopenedEllipseCenterArea = window.MeasureSelectedArea();
            Check(window.SelectedEntityId == ellipseCenterId && tree.PropertyType.Text == "ELLIPSE" &&
                tree.PropertyLength.Text == "193.77" &&
                reopenedEllipseCenterArea.Contains("Area = 2513.2741", StringComparison.Ordinal) &&
                reopenedEllipseCenterArea.Contains("Perimeter = 193.7690", StringComparison.Ordinal),
                "reopened ELLIPSE C keeps native ID, axes and exact inquiry");
            EnterCommand(window, "MEA LENGTH");
            Check((tree.StatusText.Text ?? string.Empty).Contains("Length = 193.7690", StringComparison.Ordinal),
                "reopened ELLIPSE C keeps native length inquiry");
            window.SelectAt(ellipseArcPick);
            EnterCommand(window, "MEASUREGEOM BOUNDS");
            Check(window.SelectedEntityId == ellipseArcId && tree.PropertyType.Text == "ELLIPSE" &&
                tree.PropertyLength.Text == "48.44" &&
                string.Equals(tree.StatusText.Text, ellipseArcBounds, StringComparison.Ordinal) &&
                window.LastTransactionSequence is null && !window.CanUndo && !window.CanRedo,
                "reopened ELLIPSE ARC keeps native ID, geometry and bounds without history");
            window.SelectAt(new ArcCadPoint(140, 270));
            Check(window.SelectedEntityId == splineId && tree.PropertyType.Text == "SPLINE",
                "reopened SPLINE keeps native type and selection ID");
            window.SelectAt(new ArcCadPoint(314, 250));
            Check(window.SelectedEntityId == donutId && tree.PropertyType.Text == "LWPOLYLINE" &&
                window.Entities.ToArray().Single(entity => entity.EntityId == donutId).PolyWidth == 2,
                "reopened DONUT keeps native type, selection ID and width");
            window.SelectAt(new ArcCadPoint(520, 250));
            Check(window.SelectedEntityId == pointId && tree.PropertyType.Text == "POINT" &&
                tree.PropertyLength.Text == "X 520.00 · Y 250.00",
                "reopened POINT keeps native type, selection ID and coordinates");
            window.SelectAt(cloudVisiblePick);
            Check(window.SelectedEntityId == revisionCloudId && tree.PropertyType.Text == "LWPOLYLINE",
                "reopened REVCLOUD keeps native type, selection ID and geometry");
            window.SelectAt(calligraphyPick);
            Check(window.SelectedEntityId == calligraphyCloudId && tree.PropertyType.Text == "LWPOLYLINE" &&
                window.Entities.ToArray().Single(entity => entity.EntityId == calligraphyCloudId)
                    .Vertices.Span.SequenceEqual(calligraphyCloud.Vertices.Span),
                "reopened REVCLOUD CALLIGRAPHY keeps style geometry and ID");
            window.SelectAt(convertedPick);
            Check(window.SelectedEntityId == convertedCloudId && tree.PropertyType.Text == "LWPOLYLINE" &&
                window.Entities.ToArray().All(entity => entity.EntityId != convertSourceId),
                "reopened REVCLOUD CONVERT keeps replacement ID and removes its source");
            Dispatcher.UIThread.RunJobs();
            capture = CaptureFrame(window).Png;
            Dispatcher.UIThread.RunJobs();
            Check(ColorCount(CaptureFrame(window), maskTargetPixel, Drawing, 8) == 0,
                "reopened WIPEOUT still masks prior path");
            window.SelectAt(new ArcCadPoint(440, 120));
            Check(window.SelectedEntityId == wipeoutId && tree.PropertyType.Text == "WIPEOUT",
                "reopened WIPEOUT keeps native type, selection ID and contour");
            window.SelectAt(new ArcCadPoint(615, 180));
            Check(window.SelectedEntityId == closedPolylineId && tree.PropertyType.Text == "LWPOLYLINE" &&
                window.MeasureSelectedArea().Contains("Area = 4200", StringComparison.Ordinal),
                "reopened PLINE CLOSE keeps native type, selection ID and exact AREA");
            var reopenedExplodedLines = window.Lines.ToArray()
                .Where(line => explodedLineIds.Contains(line.EntityId))
                .OrderBy(line => line.EntityId)
                .ToArray();
            Check(reopenedExplodedLines.SequenceEqual(explodedLines),
                "reopened EXPLODE pieces keep exact native IDs and geometry");
            window.SelectAt(new ArcCadPoint(710, 80));
            Check(window.SelectedEntityId == explodedTopLine.EntityId && tree.PropertyType.Text == "LINE",
                "reopened EXPLODE wall remains independently selectable");
            window.SelectAt(new ArcCadPoint(60, 340));
            Check(window.SelectedEntityId == xlineId && tree.PropertyType.Text == "XLINE",
                "reopened XLINE keeps native type and selection ID");
            window.SelectAt(new ArcCadPoint(380, 320));
            Check(window.SelectedEntityId == rayId && tree.PropertyType.Text == "RAY",
                "reopened RAY keeps native type and selection ID");
            var reopenedBeforeOopsCount = window.EntityCount;
            var reopenedBeforeOopsTransaction = window.LastTransactionSequence;
            EnterCommand(window, "OOPS");
            Check(window.IsBackendConnected && window.EntityCount == reopenedBeforeOopsCount &&
                window.LastTransactionSequence == reopenedBeforeOopsTransaction &&
                window.SelectedEntityId == rayId &&
                (tree.StatusText.Text ?? string.Empty).Contains(
                    "no hay ningún ERASE", StringComparison.Ordinal),
                "reopen clears OOPS history without changing document or selection");
            window.NewDocument();
            window.ToggleDimensions();
            window.ToggleLineweight();
            tree.Viewport.Focus();
            Dispatcher.UIThread.RunJobs();
            Check(window.Lines.Length == 0 && window.EntityCount == 0 && !window.CanUndo && !window.CanRedo,
                "house demo reset for legacy flow");
            Check(tree.Viewport.Zoom == 1 && tree.Viewport.Pan == default &&
                !tree.Viewport.ShowDimensions && !tree.Viewport.UseHeavyLineweight,
                "house demo camera and overlays reset");
            CheckPixelsEqualInside(baseFrame, CaptureFrame(window), drawingPixels, "house demo clean reset pixels");

            Click(tree.NewButton);
            Check(window.Lines.Length == 0 && window.CurrentPath is null && !window.IsDirty,
                "QAT New productive route");
            Click(tree.NewDocumentButton);
            Check(window.Lines.Length == 0 && window.CurrentPath is null && !window.IsDirty,
                "title New productive route");

            Click(tree.LineButton);
            Check(window.IsLineActive && window.IsAwaitingFirstPoint, "ribbon LINE productive route");
            window.CancelLine();
            Click(tree.RailLineButton);
            Check(window.IsLineActive && window.IsAwaitingFirstPoint, "rail LINE productive route");
            window.CancelLine();
            tree.CommandInput.Text = "L";
            tree.CommandInput.Focus();
            window.KeyPressQwerty(PhysicalKey.Enter, RawInputModifiers.None);
            Dispatcher.UIThread.RunJobs();
            Check(window.IsLineActive && window.IsAwaitingFirstPoint, "command L productive route");
            window.CancelLine();
            tree.Viewport.Focus();
            Dispatcher.UIThread.RunJobs();
            baseFrame = CaptureFrame(window);

            Click(tree.LineButton);
            Check(window.IsLineActive && window.IsAwaitingFirstPoint, "LINE awaiting first point");
            Check(!tree.LineButton.IsEnabled, "LINE button disabled while active");
            window.AcceptPoint(new ArcCadPoint(96, 96));
            Check(window.IsLineActive && !window.IsAwaitingFirstPoint, "LINE awaiting next point");
            Check(window.PendingFirstPoint == new ArcCadPoint(96, 96), "LINE pending first point");
            Check(window.Lines.Length == 0 && window.LastTransactionSequence is null, "preview has no transaction");
            Dispatcher.UIThread.RunJobs();
            Click(tree.OrthoStatus);
            Check(window.IsOrthoEnabled && tree.OrthoStatus.Classes.Contains("active"),
                "ortho enabled");
            window.MovePointer(new ArcCadPoint(320, 192));
            Dispatcher.UIThread.RunJobs();
            Check(tree.Viewport.PreviewVertices.Span.SequenceEqual([96f, 96f, 320f, 96f]),
                "ortho preview constraint");
            Click(tree.OrthoStatus);
            Check(!window.IsOrthoEnabled && !tree.OrthoStatus.Classes.Contains("active"),
                "ortho disabled");
            tree.Viewport.Focus();
            Dispatcher.UIThread.RunJobs();
            var beforeMoveFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "drawing-selection", 2, beforeMoveFrame);
            window.MovePointer(new ArcCadPoint(320, 192));
            Dispatcher.UIThread.RunJobs();
            Check(tree.Viewport.PreviewVertices.Span.SequenceEqual([96f, 96f, 320f, 192f]), "LINE preview vertices");
            Check(tree.Viewport.Lines.Length == 0 && window.LastTransactionSequence is null && !window.IsDirty,
                "preview persistent state");
            Check(tree.Viewport.HasCursor && !tree.Viewport.CursorSnapped, "preview cursor state");
            var previewFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "drawing-selection", 3, previewFrame);
            CheckDiffInside(beforeMoveFrame.Pixels, previewFrame.Pixels, previewFrame.PixelSize, viewportPixels, "preview diff");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "Escape handled");
            Dispatcher.UIThread.RunJobs();
            Check(!window.IsLineActive && window.PendingFirstPoint is null && window.LastSnap is null, "LINE cancel state");
            Check(tree.Viewport.PreviewVertices.Length == 0 && !tree.Viewport.HasCursor, "LINE cancel overlays");
            Check(window.Lines.Length == 0 && window.LastTransactionSequence is null && !window.IsDirty,
                "LINE cancel transaction state");
            var canceledFrame = CaptureFrame(window);
            CheckPixelsEqualInside(baseFrame, canceledFrame, drawingPixels, "LINE cancel restored drawing");

            window.StartLine();
            window.AcceptPoint(new ArcCadPoint(96, 96));
            Commit(window, new ArcCadPoint(360, 96), 0);
            Commit(window, new ArcCadPoint(360, 260), 1);
            Commit(window, new ArcCadPoint(96, 260), 2);
            var firstThreeIds = window.Lines.ToArray().Select(line => line.EntityId).ToArray();
            Check(firstThreeIds.Length == 3 && firstThreeIds.Distinct().Count() == 3, "first three LINE IDs");

            Click(tree.OsnapStatus);
            Check(!window.IsObjectSnapEnabled && !tree.OsnapStatus.Classes.Contains("active"),
                "object snap disabled");
            window.MovePointer(new ArcCadPoint(96.1, 96.1));
            Dispatcher.UIThread.RunJobs();
            Check(window.LastSnap is null && !tree.Viewport.CursorSnapped,
                "object snap disabled cursor");
            Check(tree.Viewport.PreviewVertices.Span.SequenceEqual([96f, 260f, 96.1f, 96.1f]),
                "object snap disabled preview");
            Click(tree.OsnapStatus);
            Check(window.IsObjectSnapEnabled && tree.OsnapStatus.Classes.Contains("active"),
                "object snap restored");
            window.MovePointer(new ArcCadPoint(96.1, 96.1));
            Dispatcher.UIThread.RunJobs();
            Check(window.LastTransactionSequence == 2 && window.Lines.Length == 3, "closing preview has no transaction");
            Check(window.LastSnap is
            {
                Point: { X: 96, Y: 96 },
                Kind: "endpoint",
                EntityId: var snapEntity,
                Distance: >= 0,
            } && snapEntity == firstThreeIds[0], "closing endpoint snap metadata");
            Check(tree.Viewport.CursorSnapped && tree.Viewport.CursorWorldPoint == new Point(96, 96), "snap cursor state");
            Check(tree.Viewport.PreviewVertices.Span.SequenceEqual([96f, 260f, 96f, 96f]), "closing preview geometry");
            var snapFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "drawing-selection", 4, snapFrame);
            var snapPixel = WorldPixel(tree, 96, 96);
            CheckColorNear(snapFrame, snapPixel + new Vector(7, 0), Amber, 2, "crosshair horizontal");
            CheckColorNear(snapFrame, snapPixel + new Vector(0, 7), Amber, 2, "crosshair vertical");
            CheckColorNear(snapFrame, snapPixel + new Vector(5, 5), Amber, 2, "snap marker corner");

            window.AcceptPoint(new ArcCadPoint(96.1, 96.1));
            Dispatcher.UIThread.RunJobs();
            Check(window.IsLineActive && !window.IsAwaitingFirstPoint, "continuous LINE remains active");
            Check(window.LastTransactionSequence == 3 && window.Lines.Length == 4 && window.IsDirty,
                "four LINE transaction state");
            Check(window.CanUndo && !window.CanRedo, "history after four commits");
            var fourLines = window.Lines.ToArray();
            var fourIds = fourLines.Select(line => line.EntityId).ToArray();
            Check(fourIds.Distinct().Count() == 4, "four unique LINE IDs");
            Check(fourLines.SequenceEqual(new[]
            {
                new CadLine(fourIds[0], 96, 96, 360, 96),
                new CadLine(fourIds[1], 360, 96, 360, 260),
                new CadLine(fourIds[2], 360, 260, 96, 260),
                new CadLine(fourIds[3], 96, 260, 96, 96),
            }), "four LINE geometry");
            Check(tree.Viewport.Lines.Length * 2 == 8, "eight persistent points");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "contour Escape handled");
            Dispatcher.UIThread.RunJobs();
            Check(!window.IsLineActive && window.PendingFirstPoint is null && window.LastSnap is null, "contour Escape state");
            Check(tree.Viewport.PreviewVertices.Length == 0 && !tree.Viewport.HasCursor && !tree.Viewport.CursorSnapped, "contour Escape overlays");
            Check(window.Lines.Span.SequenceEqual(fourLines) && window.LastTransactionSequence == 3, "Escape preserves commits");
            Click(tree.OpenButton);
            Check(tree.StatusText.Text == "Cambios sin guardar - guarde antes de continuar",
                "QAT Open dirty guard without picker");
            Check(window.Lines.Span.SequenceEqual(fourLines) && window.LastTransactionSequence == 3 && window.IsDirty,
                "QAT Open dirty guard preserves document");
            Check(window.HandleKey(Key.O, KeyModifiers.Control), "Ctrl+O handled");
            Dispatcher.UIThread.RunJobs();
            Check(tree.StatusText.Text == "Cambios sin guardar - guarde antes de continuar",
                "Ctrl+O dirty guard without picker");
            Check(window.Lines.Span.SequenceEqual(fourLines) && window.LastTransactionSequence == 3 && window.IsDirty,
                "Ctrl+O dirty guard preserves document");

            Click(tree.GridStatus);
            Check(!tree.Viewport.ShowGrid && !tree.GridStatus.Classes.Contains("active"),
                "grid toggle off route");
            Click(tree.GridStatus);
            Check(tree.Viewport.ShowGrid && tree.GridStatus.Classes.Contains("active"),
                "grid toggle on route");
            Click(tree.UcsButton);
            Check(!tree.Viewport.ShowUcs && !tree.UcsButton.Classes.Contains("active"),
                "UCS toggle off route");
            Click(tree.UcsButton);
            Check(tree.Viewport.ShowUcs && tree.UcsButton.Classes.Contains("active"),
                "UCS toggle on route");
            tree.Viewport.Focus();
            Dispatcher.UIThread.RunJobs();
            var unselectedFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "drawing-selection", 5, unselectedFrame);
            SaveDemoFrame(capturePath, "history-persistence", 2, unselectedFrame);

            tree.Viewport.ShowGrid = false;
            Dispatcher.UIThread.RunJobs();
            var noGridFrame = CaptureFrame(window);
            CheckGridDiff(unselectedFrame.Pixels, noGridFrame.Pixels, unselectedFrame.PixelSize, viewportPixels);
            tree.Viewport.ShowGrid = true;
            Dispatcher.UIThread.RunJobs();
            Check(CaptureFrame(window).Pixels.SequenceEqual(unselectedFrame.Pixels), "restored grid pixels");
            tree.Viewport.ShowUcs = false;
            Dispatcher.UIThread.RunJobs();
            var noUcsFrame = CaptureFrame(window);
            CheckUcsDiff(unselectedFrame.Pixels, noUcsFrame.Pixels, unselectedFrame.PixelSize, viewportPixels);
            tree.Viewport.ShowUcs = true;
            Dispatcher.UIThread.RunJobs();
            Check(CaptureFrame(window).Pixels.SequenceEqual(unselectedFrame.Pixels), "restored UCS pixels");

            var transactionBeforeSelection = window.LastTransactionSequence;
            window.SelectAt(new ArcCadPoint(96, 178));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == fourIds[3] && tree.Viewport.SelectedEntityId == fourIds[3], "native selected fourth LINE");
            Check(window.SelectedLine == fourLines[3], "selected LINE geometry");
            var selectedLinePath = window.Entities.ToArray()
                .Single(entity => entity.EntityId == fourIds[3]);
            Check(selectedLinePath.AnalyticLength is { } selectedLineLength &&
                Math.Abs(selectedLineLength - 164.0) < 0.000000001 &&
                selectedLinePath.Length == selectedLineLength,
                "LINE carries its native Euclidean length");
            Check(window.LastTransactionSequence == transactionBeforeSelection && window.Lines.Span.SequenceEqual(fourLines), "selection is non-mutating");
            Check(tree.Properties.IsVisible, "properties visible");
            Check(tree.PropertyType.Text == "LINE", "property type");
            Check(tree.PropertyId.Text == fourIds[3].ToString(), "property ID");
            Check(tree.PropertyLength.Text == "164.00", "property length");
            Check(
                AutomationProperties.GetName(tree.Properties) ==
                    $"Propiedades LINE, ID {fourIds[3]}, longitud 164.00",
                "properties accessible name");
            Check(!tree.PropertyType.Focusable && !tree.PropertyId.Focusable && !tree.PropertyLength.Focusable, "properties read-only");
            CheckLayout(window, tree, new Size(1672, 941));
            var selectedFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "drawing-selection", 6, selectedFrame);
            SaveDemoFrame(capturePath, "history-persistence", 3, selectedFrame);
            CheckDiffExistsInside(
                unselectedFrame.Pixels,
                selectedFrame.Pixels,
                selectedFrame.PixelSize,
                PixelBounds(BoundsIn(tree.Workspace, window)),
                "selection viewport/panel diff");
            CheckColorNear(selectedFrame, WorldPixel(tree, 96, 178), Cyan, 2, "selected LINE highlight");
            CheckColorCount(selectedFrame, WorldPixel(tree, 96, 260), Cyan, 5, 20, "first selected grip");
            CheckColorCount(selectedFrame, WorldPixel(tree, 96, 96), Cyan, 5, 20, "second selected grip");

            var queryLines = window.Lines.ToArray();
            var queryTransaction = window.LastTransactionSequence;
            var queryDirty = window.IsDirty;
            var queryCanUndo = window.CanUndo;
            var queryCanRedo = window.CanRedo;
            var querySelection = window.SelectedEntityId;
            Click(tree.ToolsRibbonTab);
            Check(!tree.Ribbon.IsVisible && tree.RibbonContext.IsVisible, "tools ribbon visible");
            Check(tree.ToolsRibbonTab.Classes.Contains("active") && !tree.HomeRibbonTab.Classes.Contains("active"),
                "tools tab active state");
            Check(tree.RibbonContext.Items.Count == 4, "tools ribbon group count");

            var distanceButton = ActionButton(window, "ribbon.herramientas.distancia");
            var angleButton = ActionButton(window, "ribbon.herramientas.angulo");
            var pointIdButton = ActionButton(window, "ribbon.herramientas.id-de-punto");
            var listButton = ActionButton(window, "ribbon.herramientas.lista");
            Check(distanceButton.IsEnabled && angleButton.IsEnabled && pointIdButton.IsEnabled && listButton.IsEnabled,
                "native query buttons enabled");

            Click(pointIdButton);
            Check(window.ActiveDrawingTool == "PointId" && window.IsAwaitingFirstPoint, "ID awaits point");
            window.AcceptPoint(new ArcCadPoint(12.5, -3.25));
            Check((tree.StatusText.Text ?? string.Empty).Contains("X = 12.5000", StringComparison.Ordinal) &&
                (tree.StatusText.Text ?? string.Empty).Contains("Y = -3.2500", StringComparison.Ordinal),
                "native ID coordinates");

            Click(distanceButton);
            window.AcceptPoint(new ArcCadPoint(0, 0));
            window.MovePointer(new ArcCadPoint(3, 4));
            Check(tree.Viewport.PreviewVertices.Span.SequenceEqual([0f, 0f, 3f, 4f]),
                "DIST transient preview");
            window.AcceptPoint(new ArcCadPoint(3, 4));
            var distanceStatus = tree.StatusText.Text ?? string.Empty;
            Check(distanceStatus.Contains("Distance = 5.0000", StringComparison.Ordinal) &&
                distanceStatus.Contains("Delta X = 3.0000", StringComparison.Ordinal) &&
                distanceStatus.Contains("Delta Y = 4.0000", StringComparison.Ordinal),
                "native 3-4-5 distance and deltas");

            Click(angleButton);
            window.AcceptPoint(new ArcCadPoint(0, 0));
            window.AcceptPoint(new ArcCadPoint(10, 0));
            window.MovePointer(new ArcCadPoint(0, 10));
            Check(tree.Viewport.PreviewVertices.Span.SequenceEqual([0f, 0f, 10f, 0f, 0f, 0f, 0f, 10f]),
                "angle two-ray preview");
            window.AcceptPoint(new ArcCadPoint(0, 10));
            Check((tree.StatusText.Text ?? string.Empty).Contains("Angle = 90.0000", StringComparison.Ordinal),
                "native right angle");

            Click(listButton);
            var listStatus = tree.StatusText.Text ?? string.Empty;
            Check(listStatus.Contains($"LINE #{fourIds[3]}", StringComparison.Ordinal) &&
                listStatus.Contains("length 164.0000", StringComparison.Ordinal),
                "native LIST selected LINE properties");
            if (capturePath is not null)
            {
                var toolsFrame = CaptureFrame(window);
                File.WriteAllBytes(
                    Path.Combine(
                        Path.GetDirectoryName(capturePath)!,
                        $"{Path.GetFileNameWithoutExtension(capturePath)}-tools.png"),
                    toolsFrame.Png);
            }
            Check(window.Lines.Span.SequenceEqual(queryLines) &&
                window.LastTransactionSequence == queryTransaction &&
                window.IsDirty == queryDirty &&
                window.CanUndo == queryCanUndo && window.CanRedo == queryCanRedo &&
                window.SelectedEntityId == querySelection,
                "all native queries are non-mutating");

            Click(distanceButton);
            window.AcceptPoint(new ArcCadPoint(1, 1));
            window.CancelLine();
            Check(tree.Viewport.PreviewVertices.Length == 0 && !window.IsLineActive &&
                window.Lines.Span.SequenceEqual(queryLines), "DIST cancel is non-mutating");

            tree.CommandInput.Text = "DI";
            tree.CommandInput.Focus();
            window.KeyPressQwerty(PhysicalKey.Enter, RawInputModifiers.None);
            Dispatcher.UIThread.RunJobs();
            Check(window.ActiveDrawingTool == "Distance" && window.IsAwaitingFirstPoint,
                "command DI reaches native distance tool");
            window.CancelLine();
            Click(tree.HomeRibbonTab);
            Check(tree.Ribbon.IsVisible && !tree.RibbonContext.IsVisible &&
                tree.HomeRibbonTab.Classes.Contains("active"), "home ribbon restored");

            window.SelectAt(new ArcCadPoint(430, 30));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId is null && tree.Viewport.SelectedEntityId is null, "empty click clears selection");
            Check(!tree.Properties.IsVisible, "empty click hides properties");
            Check(window.LastTransactionSequence == transactionBeforeSelection && window.Lines.Span.SequenceEqual(fourLines), "empty click is non-mutating");

            window.SelectAt(new ArcCadPoint(96, 178));
            window.StartLine();
            window.AcceptPoint(new ArcCadPoint(200, 140));
            window.MovePointer(new ArcCadPoint(240, 180));
            Dispatcher.UIThread.RunJobs();
            Check(window.IsLineActive && tree.Viewport.PreviewVertices.Length == 4 && tree.Viewport.HasCursor, "combined flow preview");
            Check(window.HandleKey(Key.Z, KeyModifiers.Control), "Ctrl+Z handled");
            Dispatcher.UIThread.RunJobs();
            Check(!window.IsLineActive && window.PendingFirstPoint is null && window.LastSnap is null, "undo clears FSM");
            Check(tree.Viewport.PreviewVertices.Length == 0 && !tree.Viewport.HasCursor, "undo clears visual transients");
            Check(window.Lines.Length == 3 && window.Lines.Span.SequenceEqual(fourLines.AsSpan(0, 3)), "undo removes fourth LINE");
            Check(window.SelectedEntityId is null && !tree.Properties.IsVisible, "undo clears removed selection");
            Check(window.LastTransactionSequence == 3 && window.CanUndo && window.CanRedo, "undo history/sequence state");
            Check(tree.UndoButton.IsEnabled && tree.RedoButton.IsEnabled && tree.LineButton.IsEnabled, "buttons after undo");

            Check(window.HandleKey(Key.Y, KeyModifiers.Control), "Ctrl+Y handled");
            Dispatcher.UIThread.RunJobs();
            Check(window.Lines.Span.SequenceEqual(fourLines), "redo restores IDs and geometry");
            Check(window.SelectedEntityId is null && !tree.Properties.IsVisible, "redo keeps selection clear");
            Check(window.LastTransactionSequence == 3 && window.CanUndo && !window.CanRedo, "redo history/sequence state");

            window.SelectAt(new ArcCadPoint(96, 178));
            Dispatcher.UIThread.RunJobs();
            Check(window.Lines.Length == 4 && window.SelectedEntityId == fourIds[3], "redo entity is selectable");
            Check(window.SelectedLine == fourLines[3] && tree.Viewport.SelectedEntityId == fourIds[3], "redo selection geometry");
            Check(tree.Properties.IsVisible && tree.PropertyId.Text == fourIds[3].ToString() &&
                tree.PropertyLength.Text == "164.00", "redo selection properties");
            var redoSelectedFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "drawing-selection", 7, redoSelectedFrame);
            SaveDemoFrame(capturePath, "history-persistence", 4, redoSelectedFrame);
            CheckColorNear(redoSelectedFrame, WorldPixel(tree, 96, 178), Cyan, 2, "redo selection highlight");
            CheckColorCount(redoSelectedFrame, WorldPixel(tree, 96, 260), Cyan, 5, 20, "redo first grip");
            CheckColorCount(redoSelectedFrame, WorldPixel(tree, 96, 96), Cyan, 5, 20, "redo second grip");

            window.SaveToPath(documentPath);
            Dispatcher.UIThread.RunJobs();
            var fullDocumentPath = Path.GetFullPath(documentPath);
            Check(File.Exists(documentPath) && new FileInfo(documentPath).Length > 0, "first save file");
            Check(window.CurrentPath == fullDocumentPath && !window.IsDirty, "first save state");
            var firstSaveBytes = File.ReadAllBytes(documentPath);
            CheckNoTemporaryFiles(tempDirectory, "first save temporary cleanup");

            Click(tree.UndoButton);
            Check(window.Lines.Length == 3 && window.CanRedo, "QAT Undo productive route");
            Click(tree.RedoButton);
            Dispatcher.UIThread.RunJobs();
            Check(window.Lines.Span.SequenceEqual(fourLines) && window.IsDirty && !window.CanRedo,
                "QAT Redo productive route");
            File.WriteAllBytes(documentPath, [0x53, 0x54, 0x41, 0x4c, 0x45]);
            Click(tree.SaveButton);
            Dispatcher.UIThread.RunJobs();
            Check(!window.IsDirty && window.CurrentPath == fullDocumentPath, "QAT Save productive route");
            Check(File.ReadAllBytes(documentPath).SequenceEqual(firstSaveBytes), "repeated save replaced destination");
            CheckNoTemporaryFiles(tempDirectory, "repeated save temporary cleanup");

            window.Undo();
            Dispatcher.UIThread.RunJobs();
            Check(window.IsDirty && window.Lines.Length == 3, "dirty state before close");
            var beforeCanceledClose = CaptureFrame(window);
            SaveDemoFrame(capturePath, "history-persistence", 5, beforeCanceledClose);
            var beforeCanceledLines = window.Lines.ToArray();
            window.Close();
            Dispatcher.UIThread.RunJobs();
            Check(window.IsVisible && window.IsBackendConnected, "dirty close canceled");
            Check(window.IsDirty && window.CurrentPath == fullDocumentPath &&
                window.Lines.Span.SequenceEqual(beforeCanceledLines), "dirty close preserves document");
            Check(tree.StatusText.Text == "Cambios sin guardar - guarde antes de cerrar", "dirty close status");
            var afterCanceledClose = CaptureFrame(window);
            SaveDemoFrame(capturePath, "history-persistence", 6, afterCanceledClose);
            CheckPixelsEqualInside(beforeCanceledClose, afterCanceledClose, drawingPixels, "dirty close drawing");

            window.Redo();
            Check(window.HandleKey(Key.S, KeyModifiers.Control), "Ctrl+S handled");
            Dispatcher.UIThread.RunJobs();
            Check(window.Lines.Span.SequenceEqual(fourLines) && !window.IsDirty, "Ctrl+S four-LINE save");
            CheckNoTemporaryFiles(tempDirectory, "final save temporary cleanup");

            var firstWindow = window;
            firstWindow.Close();
            Dispatcher.UIThread.RunJobs();
            Check(!firstWindow.IsVisible && !firstWindow.IsBackendConnected, "saved window closes and disposes backend");

            window = new MainWindow
            {
                WindowState = WindowState.Normal,
                Width = 1672,
                Height = 941,
            };
            window.Show();
            Dispatcher.UIThread.RunJobs();
            tree = ReadTree(window);
            CheckLayout(window, tree, new Size(1672, 941));
            CheckSingleBackend(window, tree);
            Check(window.Lines.Length == 0 && window.CurrentPath is null && !window.IsDirty, "fresh reopen window state");
            var openWarnings = window.OpenFromPath(documentPath);
            Dispatcher.UIThread.RunJobs();
            Check(openWarnings.Count == 0, "open warnings");
            Check(window.Lines.Span.SequenceEqual(fourLines), "reopen exact IDs/order/geometry");
            Check(window.CurrentPath == fullDocumentPath && !window.IsDirty, "reopen path/dirty");
            Check(window.SelectedEntityId is null && !window.CanUndo && !window.CanRedo, "reopen clears selection/history");
            Check(window.LastTransactionSequence is null && window.LineEntityId == 0, "reopen resets transaction metadata");
            tree.Viewport.Focus();
            Dispatcher.UIThread.RunJobs();
            var reopenedFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "history-persistence", 7, reopenedFrame);
            var reopenedViewportPixels = PixelBounds(BoundsIn(tree.Viewport, window));
            Check(reopenedViewportPixels == viewportPixels, "reopen viewport bounds");
            CheckPixelsEqualInside(unselectedFrame, reopenedFrame, drawingPixels, "reopen exact drawing pixels");

            window.SelectAt(new ArcCadPoint(96, 178));
            Dispatcher.UIThread.RunJobs();
            Check(window.SelectedEntityId == fourIds[3] && window.SelectedLine == fourLines[3], "reopen fourth selection");
            Check(tree.Properties.IsVisible && tree.PropertyId.Text == fourIds[3].ToString() &&
                tree.PropertyLength.Text == "164.00", "reopen properties");

            Check(window.HandleKey(Key.N, KeyModifiers.Control), "Ctrl+N handled");
            Dispatcher.UIThread.RunJobs();
            Check(window.Lines.Length == 0 && window.CurrentPath is null && !window.IsDirty, "Ctrl+N document state");
            Check(window.SelectedEntityId is null && !window.CanUndo && !window.CanRedo, "new clears selection/history");
            var newDocumentFrame = CaptureFrame(window);
            SaveDemoFrame(capturePath, "history-persistence", 8, newDocumentFrame);
            CheckPixelsEqualInside(baseFrame, newDocumentFrame, drawingPixels, "new empty drawing pixels");

            window.OpenFromPath(documentPath);
            Dispatcher.UIThread.RunJobs();
            Check(window.Lines.Span.SequenceEqual(fourLines) && !window.CanUndo && !window.CanRedo, "open after new exact state");
            window.SelectAt(new ArcCadPoint(96, 178));
            Dispatcher.UIThread.RunJobs();

            var corruptPath = Path.Combine(tempDirectory, "corrupt.arcf");
            File.WriteAllBytes(corruptPath, [0xff, 0x00, 0x41]);
            var beforeCorrupt = CaptureFrame(window);
            ExpectThrows<ArcCadCommandException>(
                () => window.OpenFromPath(corruptPath),
                "corrupt open must fail");
            Dispatcher.UIThread.RunJobs();
            Check(window.Lines.Span.SequenceEqual(fourLines) && window.CurrentPath == fullDocumentPath &&
                !window.IsDirty && window.SelectedEntityId == fourIds[3] &&
                !window.CanUndo && !window.CanRedo, "corrupt open preserves live state");
            Check(CaptureFrame(window).Pixels.SequenceEqual(beforeCorrupt.Pixels), "corrupt open preserves frame");

            var nativeCurvePath = Path.Combine(tempDirectory, "native-curve.arcf");
            using (var nativeCurveSession = ArcCadSession.Create())
            {
                var result = nativeCurveSession.ExecuteJson(
                    "CIRCLE",
                    "{\"center\":[520.0,120.0],\"radius\":20.0}");
                Check(result.StartsWith("{\"ok\":", StringComparison.Ordinal), "native CIRCLE fixture");
                File.WriteAllBytes(nativeCurvePath, nativeCurveSession.SaveArcf());
            }

            var nativeCurveWarnings = window.OpenFromPath(nativeCurvePath);
            Dispatcher.UIThread.RunJobs();
            Check(nativeCurveWarnings.Count == 0 && window.Lines.Length == 0 && window.EntityCount == 1 &&
                window.CurrentPath == Path.GetFullPath(nativeCurvePath) && !window.IsDirty &&
                !window.CanUndo && !window.CanRedo,
                "native CIRCLE-only document opens through typed render scene");
            var fixtureCircle = window.Entities.Span[0];
            window.SelectAt(new ArcCadPoint(540, 120));
            Check(window.SelectedEntityId == fixtureCircle.EntityId && tree.PropertyType.Text == "CIRCLE",
                "native CIRCLE fixture is selectable after open");
            window.OpenFromPath(documentPath);
            window.SelectAt(new ArcCadPoint(96, 178));
            Dispatcher.UIThread.RunJobs();
            Check(window.Lines.Span.SequenceEqual(fourLines) && window.CurrentPath == fullDocumentPath &&
                window.SelectedEntityId == fourIds[3] && tree.Viewport.Entities.Length == fourLines.Length,
                "LINE document restores after native curve fixture");

            window.StartLine();
            window.AcceptPoint(new ArcCadPoint(180, 140));
            window.MovePointer(new ArcCadPoint(260, 180));
            window.AcceptPoint(new ArcCadPoint(260, 180));
            Dispatcher.UIThread.RunJobs();
            var fifthLine = window.Lines.Span[4];
            Check(window.LastTransactionSequence == 0 && window.Lines.Length == 5, "reopen fifth uses txSeq 0");
            Check(window.Lines.Span[..4].SequenceEqual(fourLines), "reopen fifth preserves scene");
            Check(fifthLine.EntityId == fourIds.Max() + 1, "reopen fifth uses next persisted ID");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "reopen fifth Escape handled");

            var priorBytes = File.ReadAllBytes(documentPath);
            var failedPath = Path.Combine(tempDirectory, "missing", "failed.arcf");
            ExpectThrows<DirectoryNotFoundException>(
                () => window.SaveToPath(failedPath),
                "save to missing directory must fail");
            Check(window.IsDirty && window.CurrentPath == fullDocumentPath && window.Lines.Length == 5,
                "failed save preserves path/dirty/session");
            Check(File.ReadAllBytes(documentPath).SequenceEqual(priorBytes), "failed save preserves prior bytes");
            CheckNoTemporaryFiles(tempDirectory, "failed save temporary cleanup");
            ExpectThrows<InvalidOperationException>(() => window.NewDocument(), "dirty new must fail");
            ExpectThrows<InvalidOperationException>(() => window.OpenFromPath(documentPath), "dirty open must fail");

            window.Undo();
            Check(window.Lines.Span.SequenceEqual(fourLines) && window.CanRedo, "reopen fifth undo");
            window.Redo();
            Check(window.Lines.Length == 5 && window.Lines.Span[4] == fifthLine && !window.CanRedo, "reopen fifth redo");

            tree.LineButton.Focus();
            Dispatcher.UIThread.RunJobs();
            Check(tree.LineButton.IsFocused, "ribbon LINE focusable");
            tree.Viewport.Focus();
            Dispatcher.UIThread.RunJobs();
            Check(tree.Viewport.IsFocused, "viewport focusable");

            CheckLayout(window, tree, new Size(1672, 941));
            window.SaveToPath(documentPath);
        }
        finally
        {
            if (window is not null)
            {
                if (window.IsBackendConnected && window.IsDirty)
                {
                    window.SaveToPath(documentPath);
                }

                window.Close();
                Dispatcher.UIThread.RunJobs();
                Check(!window.IsBackendConnected, "backend disposed on close");
            }
        }

        Check(capture is not null, "final capture frame");
        Directory.Delete(tempDirectory, recursive: true);
        Check(!Directory.Exists(tempDirectory), "headless temporary directory cleanup");
        return capture!;
    }

    private static void RunPgpAliases(string? evidenceDirectory)
    {
        var tempDirectory = Path.Combine(Path.GetTempPath(), $"ArcCAD-Alias-Harness-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDirectory);
        var aliasPath = Path.Combine(tempDirectory, "Aliases.pgp");
        var documentPath = Path.Combine(tempDirectory, "pgp-alias.arcf");
        var initialPgp = string.Join("\n", [
            "; UTF-8 alias fixture",
            "línea,*LINE",
            "straße,*LINE",
            "C,*COPY",
            "RR,*RECTANG",
            "CAPA,*LAYER",
            "GW,*LINE",
            "DUP,*LINE",
            "CLOSE,*LINE",
        ]);
        File.WriteAllText(aliasPath, initialPgp, new UTF8Encoding(true, true));
        var bomBytes = File.ReadAllBytes(aliasPath);
        Check(bomBytes.Length >= 3 && bomBytes[0] == 0xEF && bomBytes[1] == 0xBB && bomBytes[2] == 0xBF,
            "PGP UTF-8 BOM fixture");

        MainWindow? window = null;
        try
        {
            window = new MainWindow(aliasPath) { Width = 1280, Height = 720 };
            window.Show();
            Dispatcher.UIThread.RunJobs();
            Check(window.AliasFilePath == Path.GetFullPath(aliasPath) && window.AliasContent == initialPgp,
                "PGP constructor loads the injected UTF-8 BOM file");

            var initialLines = window.Lines.ToArray();
            var initialTransaction = window.LastTransactionSequence;
            var initialPath = window.CurrentPath;
            var initialDirty = window.IsDirty;
            window.SubmitCommand("REINIT");
            Dispatcher.UIThread.RunJobs();
            var reinitStatus = Named<TextBlock>(window, "BackendStatusText").Text ?? string.Empty;
            Check(reinitStatus.StartsWith("REINIT - PGP:", StringComparison.Ordinal) &&
                reinitStatus.Contains("warnings", StringComparison.OrdinalIgnoreCase) &&
                reinitStatus.Contains(
                    "PGP user linea 4: alias 'C' reemplaza builtin",
                    StringComparison.Ordinal),
                "REINIT loads strict PGP and reports the permitted builtin C shadow");
            Check(window.Lines.Span.SequenceEqual(initialLines) &&
                window.LastTransactionSequence == initialTransaction &&
                window.CurrentPath == initialPath && window.IsDirty == initialDirty,
                "REINIT is document- and transaction-neutral");

            window.SubmitCommand("BAD");
            Check(window.LastCommandErrorCode == "COMMAND_ERROR" &&
                window.LastTransactionSequence == initialTransaction && window.Lines.Length == 0,
                "unknown command outside the strict PGP table is recoverable");

            window.SubmitCommand("línea");
            Check(window.ActiveDrawingTool == "Line" && window.ActiveCommand == "LINE",
                "Unicode PGP alias starts canonical LINE");
            window.AcceptPoint(new ArcCadPoint(10, 10));
            window.AcceptPoint(new ArcCadPoint(110, 10));
            Check(window.HandleKey(Key.Escape, KeyModifiers.None) && window.Lines.Length == 1 &&
                window.LastTransactionSequence == 0,
                "Unicode alias commits the canonical LINE transaction");
            window.SubmitCommand("straße");
            Check(window.ActiveDrawingTool == "Line" && window.ActiveCommand == "LINE",
                "expansive Unicode case mapping resolves through the canonical normalizer");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None),
                "expansive Unicode alias cancels cleanly");

            window.SubmitCommand("DUP");
            Check(window.ActiveDrawingTool == "Line", "single PGP alias resolves to LINE");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "single alias LINE cancels cleanly");

            window.SubmitCommand("PLINE");
            window.AcceptPoint(new ArcCadPoint(10, 30));
            window.AcceptPoint(new ArcCadPoint(110, 30));
            window.AcceptPoint(new ArcCadPoint(110, 80));
            window.SubmitCommand("CLOSE");
            Check(window.ActiveCommand is null && !window.IsLineActive && window.LastCompletedCommand == "PLINE",
                "active prompt option is not rewritten by a same-name PGP alias");

            window.SelectAt(new ArcCadPoint(60, 10));
            window.SubmitCommand("C");
            Check(window.ActiveDrawingTool == "Copy", "user PGP alias overrides builtin C to COPY");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "PGP COPY override cancels cleanly");
            Check(window.CommandHistory[^1].Command == "COPY",
                "desktop command history records the resolved canonical command");
            window.SubmitCommand("CIRCLE");
            Check(window.ActiveDrawingTool == "Circle", "canonical CIRCLE remains available beside PGP");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "canonical CIRCLE cancels cleanly");

            window.SubmitCommand("RR WIDTH 2");
            Check(window.ActiveDrawingTool == "Rectangle" && window.ActiveRectangleWidth == 2,
                "PGP replacement preserves initial command arguments");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "argument-bearing alias cancels cleanly");
            window.SubmitCommand("CAPA NEW Pgp_Mixta");
            Check(window.Layers.Any(layer => layer.Name == "Pgp_Mixta"),
                "PGP LAYER alias preserves the original argument casing");
            window.SubmitCommand("GW");
            Check(window.ActiveDrawingTool == "Line", "desktop command line uses the gateway alias");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "gateway alias LINE cancels cleanly");

            window.SaveToPath(documentPath);
            var savedLines = window.Lines.ToArray();
            var savedTransaction = window.LastTransactionSequence;
            window.SubmitCommand("ALIASEDIT ZZ,*LINE");
            Dispatcher.UIThread.RunJobs();
            var editStatus = Named<TextBlock>(window, "BackendStatusText").Text ?? string.Empty;
            Check(editStatus.Contains("ALIASEDIT - ZZ -> LINE", StringComparison.Ordinal) &&
                window.AliasContent.Contains("ZZ,*LINE", StringComparison.OrdinalIgnoreCase) &&
                File.ReadAllText(aliasPath).Contains("ZZ,*LINE", StringComparison.OrdinalIgnoreCase),
                "ALIASEDIT persists a user alias atomically");
            Check(window.Lines.Span.SequenceEqual(savedLines) &&
                window.LastTransactionSequence == savedTransaction && !window.IsDirty,
                "ALIASEDIT is document- and transaction-neutral");
            CheckNoTemporaryFiles(tempDirectory, "ALIASEDIT temporary cleanup");

            window.SubmitCommand("ALIASEDIT");
            Check((Named<TextBlock>(window, "BackendStatusText").Text ?? string.Empty)
                    .Contains(Path.GetFullPath(aliasPath), StringComparison.OrdinalIgnoreCase),
                "ALIASEDIT exposes the editable injected path");
            window.SubmitCommand("ZZ");
            Check(window.ActiveDrawingTool == "Line", "ALIASEDIT alias is active immediately");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "edited alias LINE cancels cleanly");

            window.NewDocument();
            window.SubmitCommand("ZZ");
            Check(window.ActiveDrawingTool == "Line", "NewDocument reapplies the active PGP table");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "new-document alias cancels cleanly");
            var openWarnings = window.OpenFromPath(documentPath);
            Check(openWarnings.Count == 0 && window.Lines.Span.SequenceEqual(savedLines),
                "PGP focal document reopens exactly");
            window.SubmitCommand("línea");
            Check(window.ActiveDrawingTool == "Line", "OpenFromPath reapplies the active PGP table");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "open-document alias cancels cleanly");

            window.Close();
            Dispatcher.UIThread.RunJobs();
            Check(!window.IsBackendConnected, "PGP first window closes cleanly");
            window = new MainWindow(aliasPath) { Width = 1280, Height = 720 };
            window.Show();
            Dispatcher.UIThread.RunJobs();
            window.SubmitCommand("ZZ");
            Check(window.ActiveDrawingTool == "Line", "desktop restart reloads the persisted ALIASEDIT alias");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "restart alias cancels cleanly");

            window.SubmitCommand("ALIASEDIT REMOVE ZZ");
            Dispatcher.UIThread.RunJobs();
            Check((Named<TextBlock>(window, "BackendStatusText").Text ?? string.Empty)
                    .Contains("ALIASEDIT - eliminado ZZ", StringComparison.Ordinal) &&
                !window.AliasContent.Contains("ZZ,*", StringComparison.OrdinalIgnoreCase) &&
                !File.ReadAllText(aliasPath).Contains("ZZ,*", StringComparison.OrdinalIgnoreCase),
                "ALIASEDIT REMOVE persists removal");
            window.SubmitCommand("ZZ");
            Check(window.LastCommandErrorCode == "COMMAND_ERROR" && !window.IsLineActive,
                "removed PGP alias is absent from the replacement table");

            var reopenedWarnings = window.OpenFromPath(documentPath);
            Check(reopenedWarnings.Count == 0 && window.Lines.Span.SequenceEqual(savedLines),
                "restart document fixture opens before recovery checks");
            var replacementPgp = "KEEP,*LINE\n";
            File.WriteAllText(aliasPath, replacementPgp, new UTF8Encoding(true, true));
            var beforeReplacementLines = window.Lines.ToArray();
            var beforeReplacementTransaction = window.LastTransactionSequence;
            var beforeReplacementPath = window.CurrentPath;
            window.SubmitCommand("REINIT");
            Check(window.AliasContent == replacementPgp &&
                window.Lines.Span.SequenceEqual(beforeReplacementLines) &&
                window.LastTransactionSequence == beforeReplacementTransaction &&
                window.CurrentPath == beforeReplacementPath && !window.IsDirty,
                "REINIT atomically replaces aliases without touching the document");
            window.SubmitCommand("C");
            Check(window.ActiveDrawingTool == "Circle", "replacement restores the builtin C alias");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "restored builtin C cancels cleanly");
            window.SubmitCommand("línea");
            Check(window.LastCommandErrorCode == "COMMAND_ERROR" && !window.IsLineActive,
                "alias absent from replacement content is removed");
            window.SubmitCommand("KEEP");
            Check(window.ActiveDrawingTool == "Line", "replacement alias is active");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "replacement alias cancels cleanly");

            var beforeUnknownEditContent = window.AliasContent;
            var beforeUnknownEditBytes = File.ReadAllBytes(aliasPath);
            var beforeUnknownEditLines = window.Lines.ToArray();
            var beforeUnknownEditTransaction = window.LastTransactionSequence;
            window.SubmitCommand("ALIASEDIT KEEP,*NO_SUCH_COMMAND");
            Dispatcher.UIThread.RunJobs();
            Check((Named<TextBlock>(window, "BackendStatusText").Text ?? string.Empty)
                    .StartsWith("Error ALIASEDIT -", StringComparison.Ordinal) &&
                window.AliasContent == beforeUnknownEditContent &&
                File.ReadAllBytes(aliasPath).SequenceEqual(beforeUnknownEditBytes) &&
                window.Lines.Span.SequenceEqual(beforeUnknownEditLines) &&
                window.LastTransactionSequence == beforeUnknownEditTransaction,
                "ALIASEDIT rejects an unknown target without replacing file, table, document or transaction");
            window.SubmitCommand("KEEP");
            Check(window.ActiveDrawingTool == "Line",
                "existing alias survives a rejected unknown-target edit");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None),
                "alias retained after rejected target cancels cleanly");

            var validAliasBytes = File.ReadAllBytes(aliasPath);
            var validAliasContent = window.AliasContent;
            var beforeInvalidLines = window.Lines.ToArray();
            var beforeInvalidTransaction = window.LastTransactionSequence;
            var beforeInvalidPath = window.CurrentPath;
            var beforeInvalidDirty = window.IsDirty;
            File.WriteAllBytes(aliasPath, [0xC3, 0x28]);
            window.SubmitCommand("REINIT");
            Dispatcher.UIThread.RunJobs();
            Check((Named<TextBlock>(window, "BackendStatusText").Text ?? string.Empty)
                    .StartsWith("Error REINIT -", StringComparison.Ordinal) &&
                window.AliasContent == validAliasContent &&
                window.Lines.Span.SequenceEqual(beforeInvalidLines) &&
                window.LastTransactionSequence == beforeInvalidTransaction &&
                window.CurrentPath == beforeInvalidPath && window.IsDirty == beforeInvalidDirty,
                "invalid UTF-8 REINIT preserves table, document, path, dirty state and transaction");
            Check(File.ReadAllBytes(aliasPath).SequenceEqual(new byte[] { 0xC3, 0x28 }),
                "invalid UTF-8 read leaves the source file untouched");
            window.SubmitCommand("KEEP");
            Check(window.ActiveDrawingTool == "Line", "prior alias table remains active after invalid UTF-8");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "retained alias cancels cleanly");

            File.WriteAllBytes(aliasPath, validAliasBytes);
            window.SubmitCommand("REINIT");
            Dispatcher.UIThread.RunJobs();
            Check((Named<TextBlock>(window, "BackendStatusText").Text ?? string.Empty)
                    .StartsWith("REINIT - PGP:", StringComparison.Ordinal),
                "valid PGP recovers after an invalid UTF-8 read");

            var beforeWriteFailureLines = window.Lines.ToArray();
            var beforeWriteFailureTransaction = window.LastTransactionSequence;
            var beforeWriteFailureContent = window.AliasContent;
            File.Delete(aliasPath);
            Directory.CreateDirectory(aliasPath);
            window.SubmitCommand("ALIASEDIT WRITEFAIL,*COPY");
            Dispatcher.UIThread.RunJobs();
            Check((Named<TextBlock>(window, "BackendStatusText").Text ?? string.Empty)
                    .StartsWith("Error ALIASEDIT -", StringComparison.Ordinal) &&
                Directory.Exists(aliasPath) && window.AliasContent == beforeWriteFailureContent &&
                window.Lines.Span.SequenceEqual(beforeWriteFailureLines) &&
                window.LastTransactionSequence == beforeWriteFailureTransaction,
                "failed atomic file commit rolls the runtime PGP table back without document changes");
            CheckNoTemporaryFiles(tempDirectory, "failed ALIASEDIT temporary cleanup");
            window.SubmitCommand("WRITEFAIL");
            Check(window.LastCommandErrorCode == "COMMAND_ERROR" && !window.IsLineActive,
                "failed ALIASEDIT does not leak its candidate alias");
            window.SubmitCommand("KEEP");
            Check(window.ActiveDrawingTool == "Line",
                "failed ALIASEDIT preserves the previously active alias table");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None),
                "retained alias after write failure cancels cleanly");
            Directory.Delete(aliasPath);
            File.WriteAllBytes(aliasPath, validAliasBytes);
            window.SubmitCommand("REINIT");
            Dispatcher.UIThread.RunJobs();
            if (evidenceDirectory is not null)
            {
                File.WriteAllBytes(
                    Path.Combine(evidenceDirectory, "pgp-alias.png"),
                    CaptureFrame(window).Png);
            }

            window.Close();
            Dispatcher.UIThread.RunJobs();
            Check(!window.IsBackendConnected, "PGP recovery window closes cleanly");
            window = null;

            using var nativeSession = ArcCadSession.Create();
            var typedMessage = nativeSession.ReinitializeAliases("GW,*LINE\n");
            Check(typedMessage.StartsWith("PGP: 1 alias(es) activo(s)", StringComparison.Ordinal) &&
                nativeSession.ResolveAlias("gw") == "LINE",
                "typed ArcCadSession PGP API resolves the shared gateway alias");
            var rawReinitMessage = CheckAliasAdministrativeEnvelope(
                nativeSession.ExecuteJson(
                    "__ARCFORGE_PGP_REINIT",
                    JsonSerializer.Serialize(new { pgp = "GW,*LINE\n" })),
                "raw PGP reinit envelope");
            Check(rawReinitMessage?.StartsWith("PGP: 1 alias(es) activo(s)", StringComparison.Ordinal) == true,
                "raw JSON/FFI PGP reinit uses the existing non-transactional envelope");
            Check(CheckAliasAdministrativeEnvelope(
                    nativeSession.ExecuteJson(
                        "__ARCFORGE_PGP_RESOLVE",
                        JsonSerializer.Serialize(new { token = "GW" })),
                    "raw PGP resolve envelope") == "LINE",
                "raw JSON/FFI PGP resolve returns the canonical command");

            using var aliasExecution = JsonDocument.Parse(nativeSession.ExecuteJson(
                "GW",
                JsonSerializer.Serialize(new
                {
                    p1 = new[] { 0.0, 0.0 },
                    p2 = new[] { 10.0, 0.0 },
                })));
            var aliasOk = aliasExecution.RootElement.GetProperty("ok");
            Check(aliasOk.GetProperty("txSeq").TryGetUInt64(out var aliasSequence) && aliasSequence == 0 &&
                aliasOk.GetProperty("created").GetArrayLength() == 1,
                "real FFI ExecuteJson executes the shared alias as canonical LINE without a new ABI");
        }
        finally
        {
            if (window?.IsBackendConnected == true)
            {
                if (window.IsDirty)
                {
                    window.SaveToPath(documentPath);
                }

                window.Close();
                Dispatcher.UIThread.RunJobs();
            }

            Directory.Delete(tempDirectory, recursive: true);
        }
    }

    private static string? CheckAliasAdministrativeEnvelope(string json, string message)
    {
        using var document = JsonDocument.Parse(json);
        var root = document.RootElement;
        Check(root.ValueKind == JsonValueKind.Object && root.EnumerateObject().Count() == 1 &&
            root.TryGetProperty("ok", out _),
            message);
        var ok = root.GetProperty("ok");
        Check(ok.ValueKind == JsonValueKind.Object &&
            ok.TryGetProperty("txSeq", out var transaction) && transaction.ValueKind == JsonValueKind.Null &&
            ok.TryGetProperty("created", out var created) && created.ValueKind == JsonValueKind.Array &&
            created.GetArrayLength() == 0,
            message);
        return ok.TryGetProperty("message", out var result) && result.ValueKind == JsonValueKind.String
            ? result.GetString()
            : null;
    }

    private static void RunCommandSessionContract()
    {
        var session = new CommandSession();
        var noRepeat = ExpectThrows<CommandInputException>(
            () => session.ResolveInput(""),
            "blank input without history must fail");
        Check(noRepeat.Code == "NO_REPEAT", "repeat error is typed");

        session.Begin(
            " pline ",
            "Seleccione el primer punto",
            ["close", "finish"],
            "finish",
            "preview");
        Check(session.ActiveCommand == "PLINE", "command normalized");
        Check(session.ResolveInput(" close ") == "CLOSE", "keyboard option normalized");
        Check(session.ResolveInput("c") == "CLOSE", "unique keyboard option prefix resolved");
        Check(session.ResolveInput("") == "FINISH", "active default resolved");
        var badOption = ExpectThrows<CommandInputException>(
            () => session.ResolveOption("bad"),
            "unknown option must fail");
        Check(badOption.Code == "INVALID_OPTION" && session.IsActive, "invalid option preserves active command");
        session.RejectInput(badOption.Message);
        session.MarkProgress("segment committed");
        Check(session.Cancel("finished by cancel"), "first cancel handled");
        Check(!session.Cancel(), "cancel is idempotent");
        Check(session.LastCompletedCommand == "PLINE", "committed cancel enables repeat");
        Check(session.History[^1].Outcome == CommandOutcome.Completed, "committed cancel is completed");
        Check(session.ResolveInput("") == "PLINE", "blank input repeats last completed command");

        session.Begin("LINE", "first", preview: "line preview");
        Check(session.Cancel(), "empty command cancel handled");
        Check(session.History[^1].Outcome == CommandOutcome.Cancelled, "empty cancel recorded");
        session.Fail("BOGUS", "unknown command");
        Check(session.History[^1].Outcome == CommandOutcome.Error, "error recorded");
        Check(session.LastError?.Code == "COMMAND_ERROR", "error remains typed");
        session.Begin("LINE", "recovered");
        session.Complete("done");
        Check(session.LastCompletedCommand == "LINE", "session reusable after error");
        Check(session.PreviousHistoryCommand() == "LINE", "history previous");
        Check(session.NextHistoryCommand() is null, "history next reaches live input");
    }

    private static void RunCommandSessionUi()
    {
        var tempDirectory = Path.Combine(Path.GetTempPath(), $"ArcCAD-Command-Session-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDirectory);
        var documentPath = Path.Combine(tempDirectory, "command-session.arcf");
        MainWindow? window = null;
        try
        {
            window = new MainWindow { Width = 1280, Height = 720 };
            window.Show();
            Dispatcher.UIThread.RunJobs();
            Click(Named<Button>(window, "OsnapStatusButton"));

            window.SubmitCommand("");
            Check(window.ActiveCommand is null && window.LastCommandErrorCode == "NO_REPEAT" &&
                window.CommandHistory.Count == 0,
                "blank Enter without history is typed and non-terminal");

            window.SubmitCommand("PLINE");
            Check(window.ActiveCommand == "PLINE", "UI command active");
            Check(window.CommandDefault == "FINISH", "UI command default");
            Check(window.CommandOptions.SequenceEqual(["CLOSE", "FINISH"]), "UI command options");
            Check(window.CommandPreview is not null && !string.IsNullOrWhiteSpace(window.CommandPrompt),
                "UI prompt and preview observable");
            window.AcceptPoint(new ArcCadPoint(0, 0));
            window.AcceptPoint(new ArcCadPoint(10, 0));
            window.AcceptPoint(new ArcCadPoint(10, 10));
            window.SubmitCommand("CLOSE");
            Check(window.ActiveCommand is null && window.LastCompletedCommand == "PLINE", "keyboard CLOSE completes");
            var afterKeyboardClose = window.EntityCount;

            window.SubmitCommand("");
            Check(window.ActiveCommand == "PLINE", "blank Enter repeats completed command");
            window.AcceptPoint(new ArcCadPoint(20, 0));
            window.AcceptPoint(new ArcCadPoint(30, 0));
            window.SubmitCommand("");
            Check(window.ActiveCommand is null && window.EntityCount == afterKeyboardClose + 1,
                "blank Enter confirms active default");

            window.SubmitCommand("PLINE");
            window.AcceptPoint(new ArcCadPoint(40, 0));
            window.AcceptPoint(new ArcCadPoint(50, 0));
            window.AcceptPoint(new ArcCadPoint(50, 10));
            Click(ActionButton(window, "command.options"));
            Check(window.ActiveCommand is null && window.EntityCount == afterKeyboardClose + 2,
                "clickable CLOSE uses command option path");

            window.SubmitCommand("");
            var beforeCancel = window.EntityCount;
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "Escape cancels repeated command");
            Check(window.ActiveCommand is null && window.CommandPreview is null && window.EntityCount == beforeCancel,
                "cancel clears preview without mutation");
            Check(!window.HandleKey(Key.Escape, KeyModifiers.None), "second Escape is idempotent");

            window.SubmitCommand("PLINE");
            window.AcceptPoint(new ArcCadPoint(60, 0));
            window.SubmitCommand("");
            Check(window.ActiveCommand == "PLINE" && window.LastCommandErrorCode == "INVALID_INPUT",
                "incomplete default is typed and keeps command active");
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "invalid command remains cancellable");

            window.SubmitCommand("NOT_A_COMMAND");
            Check(window.ActiveCommand is null && window.LastCommandErrorCode == "COMMAND_ERROR",
                "unknown command is typed and terminal");
            window.SubmitCommand("LINE");
            Check(window.ActiveCommand == "LINE", "session recovers after error");
            window.AcceptPoint(new ArcCadPoint(70, 0));
            window.AcceptPoint(new ArcCadPoint(80, 0));
            Check(window.HandleKey(Key.Escape, KeyModifiers.None), "continuous LINE ends with Escape");
            Check(window.CommandHistory[^1].Outcome == CommandOutcome.Completed, "completed LINE recorded");

            Click(ActionButton(window, "command.previous"));
            Check(Named<TextBox>(window, "CommandInput").Text == "LINE", "history previous populates input");
            Click(ActionButton(window, "command.next"));
            Check(string.IsNullOrEmpty(Named<TextBox>(window, "CommandInput").Text), "history next returns live input");

            window.SaveToPath(documentPath);
            window.Close();
            Dispatcher.UIThread.RunJobs();
            Check(!window.IsBackendConnected, "command session window disposed");
        }
        finally
        {
            if (window?.IsBackendConnected == true)
            {
                if (window.IsDirty)
                {
                    window.SaveToPath(documentPath);
                }

                window.Close();
                Dispatcher.UIThread.RunJobs();
            }

            Directory.Delete(tempDirectory, recursive: true);
        }
    }

    private static void RunLayerLifecycle()
    {
        var tempDirectory = Path.Combine(Path.GetTempPath(), $"ArcCAD-Layer-Lifecycle-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDirectory);
        var documentPath = Path.Combine(tempDirectory, "layers.arcf");
        MainWindow? window = null;
        try
        {
            window = new MainWindow { Width = 1280, Height = 720 };
            window.Show();
            Dispatcher.UIThread.RunJobs();
            Click(Named<Button>(window, "OsnapStatusButton"));

            var zero = window.Layers.Single();
            Check(zero.Id != 0 && zero.Name == "0" && zero.Current && zero.Plot &&
                !zero.Off && !zero.Frozen && !zero.Locked,
                "LAYER initial native catalog");

            ExecuteLayerMutation(window, "LAYER NEW A-WALL", "create layer");
            var wall = window.Layers.Single(layer => layer.Name == "A-WALL");
            var wallId = wall.Id;
            Check(wallId != zero.Id && window.SelectedLayerId == wallId && !wall.Current && wall.Plot &&
                !wall.Off && !wall.Frozen && !wall.Locked,
                "LAYER create stable ID and defaults");
            CheckRejectedLayerCommand(window, "LAYER NEW A-WALL", "duplicate layer");

            ExecuteLayerMutation(window, $"LAYER RENAME {wallId} A-MUROS", "rename layer");
            Check(window.Layers.Single(layer => layer.Id == wallId).Name == "A-MUROS",
                "LAYER rename preserves ID");
            window.Undo();
            Check(window.Layers.Single(layer => layer.Id == wallId).Name == "A-WALL" && window.CanRedo,
                "LAYER rename undo restores name and ID");
            window.Redo();
            Check(window.Layers.Single(layer => layer.Id == wallId).Name == "A-MUROS" && !window.CanRedo,
                "LAYER rename redo restores name and ID");

            var zeroLineId = CreateLine(window, new ArcCadPoint(10, 30), new ArcCadPoint(110, 30));
            var zeroLine = FindLine(window, zeroLineId);
            ExecuteLayerMutation(window, $"LAYER SET-CURRENT {wallId}", "set current layer");
            Check(window.Layers.Single(layer => layer.Id == wallId).Current &&
                !window.Layers.Single(layer => layer.Id == zero.Id).Current,
                "LAYER set-current exact catalog");
            var lineId = CreateLine(window, new ArcCadPoint(1000, 10), new ArcCadPoint(1100, 10));
            var line = FindLine(window, lineId);
            Check(window.LastTransactionSequence == 4 && window.Entities.Length == 2,
                "new LINE uses current layer in one transaction and preserves the other layer batch");
            window.FitView();
            var bothLayerZoom = Named<CadViewport>(window, "WorkspaceViewport").Zoom;
            Check(bothLayerZoom < 2, "visible layer batches both participate in viewport extents");

            ExecuteLayerMutation(window, $"LAYER OFF {wallId}", "layer off");
            Check(window.Lines.Length == 1 && window.Entities.Length == 1 &&
                FindLine(window, zeroLineId) == zeroLine,
                "LAYER off removes only its own render batch");
            window.SelectAt(new ArcCadPoint(1050, 10));
            Check(window.SelectedEntityId is null, "LAYER off removes entity from hit testing");
            window.FitView();
            Check(Named<CadViewport>(window, "WorkspaceViewport").Zoom > bothLayerZoom * 2,
                "LAYER off removes its geometry from viewport extents");
            ExecuteLayerMutation(window, $"LAYER ON {wallId}", "layer on");
            Check(FindLine(window, lineId) == line && FindLine(window, zeroLineId) == zeroLine,
                "LAYER on restores exact entity ID and both render batches");
            window.FitView();
            Check(Math.Abs(Named<CadViewport>(window, "WorkspaceViewport").Zoom - bothLayerZoom) < 0.001,
                "LAYER on restores its exact contribution to viewport extents");

            ExecuteLayerMutation(window, $"LAYER FREEZE {wallId}", "layer freeze");
            Check(window.Lines.Length == 1 && window.Entities.Length == 1 &&
                FindLine(window, zeroLineId) == zeroLine,
                "LAYER freeze removes only its own render batch");
            window.SelectAt(new ArcCadPoint(1050, 10));
            Check(window.SelectedEntityId is null, "LAYER freeze removes entity from hit testing");
            window.FitView();
            Check(Named<CadViewport>(window, "WorkspaceViewport").Zoom > bothLayerZoom * 2,
                "LAYER freeze removes its geometry from viewport extents");
            ExecuteLayerMutation(window, $"LAYER THAW {wallId}", "layer thaw");
            Check(FindLine(window, lineId) == line && FindLine(window, zeroLineId) == zeroLine,
                "LAYER thaw restores exact entity ID and both render batches");
            window.FitView();
            Check(Math.Abs(Named<CadViewport>(window, "WorkspaceViewport").Zoom - bothLayerZoom) < 0.001,
                "LAYER thaw restores its exact contribution to viewport extents");

            var sceneBeforePlot = window.Lines.ToArray();
            ExecuteLayerMutation(window, $"LAYER NO-PLOT {wallId}", "layer no-plot");
            Check(!window.Layers.Single(layer => layer.Id == wallId).Plot &&
                window.Lines.Span.SequenceEqual(sceneBeforePlot),
                "LAYER no-plot changes catalog without changing scene");
            window.SubmitCommand("LAYER");
            Dispatcher.UIThread.RunJobs();
            Check(Named<Border>(window, "LayerManagerPanel").IsVisible,
                "LAYER command exposes real layer UI");
            var beforeUiPlotTransaction = window.LastTransactionSequence;
            Click(ActionButton(window, $"layers.plot.{wallId}"));
            CheckTransactionAdvanced(window, beforeUiPlotTransaction, "layer UI plot");
            Check(window.Layers.Single(layer => layer.Id == wallId).Plot &&
                window.Lines.Span.SequenceEqual(sceneBeforePlot),
                "layer UI plot uses native gateway without changing scene");
            ExecuteLayerMutation(window, $"LAYER NO-PLOT {wallId}", "persist no-plot");

            var layerNameBox = Named<TextBox>(window, "LayerSearchBox");
            layerNameBox.Text = "UI-TEMP";
            var beforeUiCreate = window.LastTransactionSequence;
            Click(ActionButton(window, "layers.new"));
            CheckTransactionAdvanced(window, beforeUiCreate, "layer UI new");
            var uiLayerId = window.Layers.Single(layer => layer.Name == "UI-TEMP").Id;
            layerNameBox.Text = "UI-RENAMED";
            var beforeUiRename = window.LastTransactionSequence;
            Click(ActionButton(window, "layers.rename"));
            CheckTransactionAdvanced(window, beforeUiRename, "layer UI rename");
            Check(window.Layers.Single(layer => layer.Id == uiLayerId).Name == "UI-RENAMED",
                "layer UI rename preserves native ID");
            var beforeUiDelete = window.LastTransactionSequence;
            Click(ActionButton(window, "layers.delete"));
            CheckTransactionAdvanced(window, beforeUiDelete, "layer UI delete");
            Check(window.Layers.All(layer => layer.Id != uiLayerId),
                "layer UI delete updates the native catalog");

            ExecuteLayerMutation(window, $"LAYER LOCK {wallId}", "layer lock");
            Check(FindLine(window, lineId) == line, "LAYER lock keeps entity visible");
            window.SelectAt(new ArcCadPoint(1050, 10));
            Check(window.SelectedEntityId == lineId, "LAYER lock keeps entity selectable");
            var beforeLockedMoveTransaction = window.LastTransactionSequence;
            var beforeLockedMoveLayers = window.Layers.ToArray();
            window.StartMove();
            window.AcceptPoint(new ArcCadPoint(1000, 10));
            ExpectThrows<ArcCadCommandException>(
                () => window.AcceptPoint(new ArcCadPoint(1010, 20)),
                "locked layer must reject MOVE");
            Check(window.IsBackendConnected && window.LastTransactionSequence == beforeLockedMoveTransaction &&
                window.Layers.SequenceEqual(beforeLockedMoveLayers) && FindLine(window, lineId) == line,
                "locked mutation rejection preserves session, catalog, transaction and scene");
            window.CancelLine();

            ExecuteLayerMutation(window, "LAYER NEW B-EMPTY", "create empty layer");
            var firstEmptyId = window.Layers.Single(layer => layer.Name == "B-EMPTY").Id;
            ExecuteLayerMutation(window, $"LAYER DELETE {firstEmptyId}", "delete empty layer");
            Check(window.Layers.All(layer => layer.Id != firstEmptyId), "LAYER deletes empty layer");
            ExecuteLayerMutation(window, "LAYER NEW B-EMPTY", "recreate empty layer");
            var currentId = window.Layers.Single(layer => layer.Name == "B-EMPTY").Id;
            Check(currentId != firstEmptyId, "LAYER recreate receives a new stable ID");
            var beforeUiCurrent = window.LastTransactionSequence;
            Named<ComboBox>(window, "CurrentLayerCombo").SelectedIndex = window.Layers
                .Select((layer, index) => (layer, index))
                .Single(item => item.layer.Id == currentId)
                .index;
            Dispatcher.UIThread.RunJobs();
            CheckTransactionAdvanced(window, beforeUiCurrent, "layer UI set-current");
            Check(window.Layers.Single(layer => layer.Id == currentId).Current,
                "layer UI current combo follows the native catalog");

            CheckRejectedLayerCommand(window, $"LAYER DELETE {zero.Id}", "delete layer 0");
            CheckRejectedLayerCommand(window, $"LAYER DELETE {currentId}", "delete current layer");
            CheckRejectedLayerCommand(window, $"LAYER DELETE {wallId}", "delete in-use layer");

            ExecuteLayerMutation(window, $"LAYER OFF {wallId}", "off non-current layer");
            CheckRejectedLayerCommand(window, $"LAYER SET-CURRENT {wallId}", "set-current off layer");
            ExecuteLayerMutation(window, $"LAYER ON {wallId}", "restore non-current layer");
            ExecuteLayerMutation(window, $"LAYER FREEZE {wallId}", "freeze non-current layer");
            CheckRejectedLayerCommand(window, $"LAYER SET-CURRENT {wallId}", "set-current frozen layer");
            ExecuteLayerMutation(window, $"LAYER THAW {wallId}", "thaw non-current layer");

            var persistedLayers = window.Layers.ToArray();
            Check(persistedLayers.Single(layer => layer.Id == wallId) is
                { Name: "A-MUROS", Locked: true, Plot: false, Off: false, Frozen: false, Current: false } &&
                persistedLayers.Single(layer => layer.Id == currentId).Current,
                "LAYER persistence fixture catalog");
            window.SaveToPath(documentPath);
            Check(!window.IsDirty && File.Exists(documentPath), "LAYER save fixture");
            window.Close();
            Dispatcher.UIThread.RunJobs();
            Check(!window.IsBackendConnected, "LAYER saved window disposed");

            window = new MainWindow { Width = 1280, Height = 720 };
            window.Show();
            Dispatcher.UIThread.RunJobs();
            var warnings = window.OpenFromPath(documentPath);
            Dispatcher.UIThread.RunJobs();
            Check(warnings.Count == 0 && window.Layers.SequenceEqual(persistedLayers),
                "LAYER reopen preserves order, IDs, flags and current layer");
            Check(window.Lines.Length == 2 && FindLine(window, lineId) == line &&
                FindLine(window, zeroLineId) == zeroLine &&
                window.LastTransactionSequence is null && !window.CanUndo && !window.CanRedo,
                "LAYER reopen preserves scene and clears session history");
            ExecuteLayerMutation(window, $"LAYER UNLOCK {wallId}", "reopen layer recovery");
            Check(window.LastTransactionSequence == 0 &&
                !window.Layers.Single(layer => layer.Id == wallId).Locked,
                "LAYER reopened session remains productive with txSeq 0");
        }
        finally
        {
            if (window?.IsBackendConnected == true)
            {
                if (window.IsDirty)
                {
                    window.SaveToPath(documentPath);
                }

                window.Close();
                Dispatcher.UIThread.RunJobs();
            }

            Directory.Delete(tempDirectory, recursive: true);
        }
    }

    private static void ExecuteLayerMutation(MainWindow window, string command, string message)
    {
        var before = window.LastTransactionSequence;
        window.SubmitCommand(command);
        Dispatcher.UIThread.RunJobs();
        CheckTransactionAdvanced(window, before, message);
    }

    private static void CheckTransactionAdvanced(MainWindow window, ulong? before, string message)
    {
        var expected = before is { } sequence ? checked(sequence + 1) : 0;
        Check(window.IsBackendConnected && window.LastTransactionSequence == expected,
            $"{message} commits exactly one native transaction");
    }

    private static void CheckRejectedLayerCommand(MainWindow window, string command, string message)
    {
        var layers = window.Layers.ToArray();
        var lines = window.Lines.ToArray();
        var transaction = window.LastTransactionSequence;
        var canUndo = window.CanUndo;
        var canRedo = window.CanRedo;
        var dirty = window.IsDirty;
        var selectedEntityId = window.SelectedEntityId;
        window.SubmitCommand(command);
        Dispatcher.UIThread.RunJobs();
        Check(window.IsBackendConnected && window.LastTransactionSequence == transaction &&
            window.Layers.SequenceEqual(layers) && window.Lines.Span.SequenceEqual(lines) &&
            window.CanUndo == canUndo && window.CanRedo == canRedo && window.IsDirty == dirty &&
            window.SelectedEntityId == selectedEntityId,
            $"{message} is non-mutating and recoverable");
        Check((Named<TextBlock>(window, "BackendStatusText").Text ?? string.Empty)
                .StartsWith("Error LAYER", StringComparison.Ordinal),
            $"{message} reports an honest error");
    }

    private static void RunPostCommitFault()
    {
        MainWindow? window = null;
        try
        {
            window = new MainWindow
            {
                WindowState = WindowState.Normal,
                Width = 1280,
                Height = 720,
            };
            window.Show();
            Dispatcher.UIThread.RunJobs();
            var sessionField = typeof(MainWindow).GetField("_session", BindingFlags.Instance | BindingFlags.NonPublic);
            Check(sessionField is not null, "fault reflection session field");
            var workspace = sessionField!.GetValue(window);
            Check(workspace is not null, "fault reflection workspace");
            var sequenceField = workspace!.GetType().GetField(
                "_nextTransactionSequence",
                BindingFlags.Instance | BindingFlags.NonPublic);
            Check(sequenceField is not null, "fault reflection sequence field");
            sequenceField!.SetValue(workspace, 99UL);

            window.StartLine();
            window.AcceptPoint(new ArcCadPoint(40, 40));
            ExpectThrows<InvalidOperationException>(
                () => window.AcceptPoint(new ArcCadPoint(120, 40)),
                "post-commit transaction mismatch must fail");
            Check(!window.IsBackendConnected, "post-commit fault disconnects backend");
            ExpectThrows<ObjectDisposedException>(
                () => window.MovePointer(new ArcCadPoint(130, 40)),
                "post-commit fault rejects reuse");
        }
        finally
        {
            if (window is not null)
            {
                window.Close();
                Dispatcher.UIThread.RunJobs();
                Check(!window.IsBackendConnected, "fault backend disposed on close");
            }
        }
    }

    private static void RunEraseLastLine()
    {
        var tempDirectory = Path.Combine(Path.GetTempPath(), $"ArcCAD-Erase-Last-Line-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDirectory);
        var path = Path.Combine(tempDirectory, "empty-after-erase.arcf");
        MainWindow? window = null;
        try
        {
            window = new MainWindow { Width = 1280, Height = 720 };
            window.Show();
            Dispatcher.UIThread.RunJobs();
            window.StartLine();
            window.AcceptPoint(new ArcCadPoint(20, 20));
            window.AcceptPoint(new ArcCadPoint(120, 20));
            window.CancelLine();
            var entityId = window.LineEntityId;
            window.SelectAt(new ArcCadPoint(70, 20));
            window.EraseSelectedEntity();
            Check(window.Lines.Length == 0 && window.SelectedEntityId is null &&
                window.LastTransactionSequence == 1, "ERASE last LINE removes native batch");
            window.Undo();
            Check(window.Lines.Length == 1 && window.Lines.Span[0].EntityId == entityId,
                "ERASE last LINE undo restores batch and ID");
            window.Redo();
            Check(window.Lines.Length == 0, "ERASE last LINE redo removes batch");
            window.SaveToPath(path);
            window.Close();
            Dispatcher.UIThread.RunJobs();
            Check(!window.IsBackendConnected, "ERASE last LINE window disposed");
        }
        finally
        {
            if (window?.IsBackendConnected == true)
            {
                if (window.IsDirty)
                {
                    window.SaveToPath(path);
                }

                window.Close();
                Dispatcher.UIThread.RunJobs();
            }

            Directory.Delete(tempDirectory, recursive: true);
        }
    }

    private static void RunNativeEditCommands()
    {
        var tempDirectory = Path.Combine(Path.GetTempPath(), $"ArcCAD-Native-Edit-A-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDirectory);
        try
        {
            RunEditCase(tempDirectory, "scale", window =>
            {
                var sourceId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                var source = FindLine(window, sourceId);
                window.SelectAt(new ArcCadPoint(5, 0));

                EnterCommand(window, "SC 2");
                Check(window.ActiveDrawingTool == "Scale", "SC alias starts native SCALE");
                window.CancelLine();
                Check(!window.IsLineActive && window.LastTransactionSequence == 0 && FindLine(window, sourceId) == source,
                    "SCALE cancellation creates no transaction");

                EnterCommand(window, "SCALE 2");
                window.AcceptPoint(new ArcCadPoint(0, 0));
                var scaled = FindLine(window, sourceId);
                Check(LineMatches(scaled, 0, 0, 20, 0) && window.Lines.Length == 1 &&
                    window.LastTransactionSequence == 1,
                    "SCALE native preserves ID and commits one transaction");
                window.Undo();
                Check(FindLine(window, sourceId) == source, "SCALE native undo restores source");
                window.Redo();
                Check(FindLine(window, sourceId) == scaled, "SCALE native redo restores result");
            });

            RunEditCase(tempDirectory, "offset", window =>
            {
                var sourceId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                window.SelectAt(new ArcCadPoint(5, 0));
                EnterCommand(window, "O 3");
                Check(window.ActiveDrawingTool == "Offset", "O alias starts native OFFSET");
                window.AcceptPoint(new ArcCadPoint(5, -20));

                var offset = window.Lines.ToArray().Single(line => line.EntityId != sourceId);
                Check(LineMatches(offset, 0, -3, 10, -3) && window.Lines.Length == 2 &&
                    window.LastTransactionSequence == 1,
                    "OFFSET native creates parallel LINE with new ID in one transaction");
                window.Undo();
                Check(window.Lines.Length == 1 && FindLine(window, sourceId).EntityId == sourceId,
                    "OFFSET native undo removes created ID");
                window.Redo();
                Check(window.Lines.Length == 2 && FindLine(window, offset.EntityId) == offset,
                    "OFFSET native redo restores created ID");
            });

            RunEditCase(tempDirectory, "trim", window =>
            {
                var targetId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                _ = CreateLine(window, new ArcCadPoint(4, -5), new ArcCadPoint(4, 5));
                window.SelectAt(new ArcCadPoint(1, 0));
                EnterCommand(window, "TR");
                Check(window.ActiveDrawingTool == "Trim", "TR alias starts native TRIM");
                window.AcceptPoint(new ArcCadPoint(1, 0));

                var trimmed = FindLine(window, targetId);
                Check(LineMatches(trimmed, 4, 0, 10, 0) && window.Lines.Length == 2 &&
                    window.LastTransactionSequence == 2,
                    "TRIM quick mode uses visible boundary and preserves target ID");
                window.Undo();
                Check(LineMatches(FindLine(window, targetId), 0, 0, 10, 0),
                    "TRIM native undo restores target");
                window.Redo();
                Check(FindLine(window, targetId) == trimmed, "TRIM native redo restores cut");
            });

            RunEditCase(tempDirectory, "extend", window =>
            {
                var targetId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(5, 0));
                _ = CreateLine(window, new ArcCadPoint(10, -5), new ArcCadPoint(10, 5));
                _ = CreateLine(window, new ArcCadPoint(15, -5), new ArcCadPoint(15, 5));
                window.SelectAt(new ArcCadPoint(2, 0));
                EnterCommand(window, "EX");
                Check(window.ActiveDrawingTool == "Extend", "EX alias starts native EXTEND");
                window.AcceptPoint(new ArcCadPoint(5, 0));

                var extended = FindLine(window, targetId);
                Check(LineMatches(extended, 0, 0, 10, 0) && window.Lines.Length == 3 &&
                    window.LastTransactionSequence == 3,
                    "EXTEND quick mode chooses nearest visible boundary and preserves target ID");
                window.Undo();
                Check(LineMatches(FindLine(window, targetId), 0, 0, 5, 0),
                    "EXTEND native undo restores target");
                window.Redo();
                Check(FindLine(window, targetId) == extended, "EXTEND native redo restores extension");
            });
        }
        finally
        {
            Directory.Delete(tempDirectory, recursive: true);
        }
    }

    private static void RunNativeEditCommandsII()
    {
        var tempDirectory = Path.Combine(Path.GetTempPath(), $"ArcCAD-Native-Edit-B-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDirectory);
        try
        {
            RunEditCase(tempDirectory, "chamfer", window =>
            {
                var firstId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                var secondId = CreateLine(window, new ArcCadPoint(0, 2), new ArcCadPoint(0, 10));
                window.SelectAt(new ArcCadPoint(8, 0));
                EnterCommand(window, "CHA 3");
                Check(window.ActiveDrawingTool == "Chamfer", "CHA alias starts native CHAMFER");
                window.AcceptPoint(new ArcCadPoint(0, 6));

                var bevel = window.Lines.ToArray().Single(
                    line => line.EntityId != firstId && line.EntityId != secondId);
                Check(LineMatches(FindLine(window, firstId), 3, 0, 10, 0) &&
                    LineMatches(FindLine(window, secondId), 0, 3, 0, 10) &&
                    LineMatches(bevel, 3, 0, 0, 3) && window.Lines.Length == 3 &&
                    window.LastTransactionSequence == 2,
                    "CHAMFER native trims two IDs and creates one bevel in one transaction");
                window.Undo();
                Check(LineMatches(FindLine(window, firstId), 0, 0, 10, 0) &&
                    LineMatches(FindLine(window, secondId), 0, 2, 0, 10) && window.Lines.Length == 2,
                    "CHAMFER native undo restores both sources");
                window.Redo();
                Check(window.Lines.Length == 3 && FindLine(window, bevel.EntityId) == bevel,
                    "CHAMFER native redo restores bevel ID");
            });

            RunEditCase(tempDirectory, "break", window =>
            {
                var sourceId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                window.SelectAt(new ArcCadPoint(5, 0));
                EnterCommand(window, "BR");
                Check(window.ActiveDrawingTool == "Break", "BR alias starts native BREAK");
                window.AcceptPoint(new ArcCadPoint(3, 0));
                window.AcceptPoint(new ArcCadPoint(7, 0));

                var tail = window.Lines.ToArray().Single(line => line.EntityId != sourceId);
                Check(LineMatches(FindLine(window, sourceId), 0, 0, 3, 0) &&
                    LineMatches(tail, 7, 0, 10, 0) && window.Lines.Length == 2 &&
                    window.LastTransactionSequence == 1,
                    "BREAK native removes middle span and creates tail in one transaction");
                window.Undo();
                Check(LineMatches(FindLine(window, sourceId), 0, 0, 10, 0) && window.Lines.Length == 1,
                    "BREAK native undo restores whole source");
                window.Redo();
                Check(window.Lines.Length == 2 && FindLine(window, tail.EntityId) == tail,
                    "BREAK native redo restores gap and tail ID");
            });

            RunEditCase(tempDirectory, "break-at-point", window =>
            {
                var sourceId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                window.SelectAt(new ArcCadPoint(5, 0));
                EnterCommand(window, "BREAKATPOINT");
                Check(window.ActiveDrawingTool == "BreakAtPoint", "BREAKATPOINT route active");
                window.AcceptPoint(new ArcCadPoint(4, 0));

                var second = window.Lines.ToArray().Single(line => line.EntityId != sourceId);
                Check(LineMatches(FindLine(window, sourceId), 0, 0, 4, 0) &&
                    LineMatches(second, 4, 0, 10, 0) && window.Lines.Length == 2 &&
                    window.LastTransactionSequence == 1,
                    "BREAKATPOINT native splits without gap and preserves source ID");
                window.Undo();
                Check(LineMatches(FindLine(window, sourceId), 0, 0, 10, 0) && window.Lines.Length == 1,
                    "BREAKATPOINT native undo restores source");
                window.Redo();
                Check(window.Lines.Length == 2 && FindLine(window, second.EntityId) == second,
                    "BREAKATPOINT native redo restores second ID");
            });

            RunEditCase(tempDirectory, "lengthen", window =>
            {
                var sourceId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                window.SelectAt(new ArcCadPoint(5, 0));
                EnterCommand(window, "LEN 15");
                Check(window.ActiveDrawingTool == "Lengthen", "LEN alias starts native LENGTHEN");
                window.AcceptPoint(new ArcCadPoint(9, 0));

                var lengthened = FindLine(window, sourceId);
                Check(LineMatches(lengthened, 0, 0, 15, 0) && window.Lines.Length == 1 &&
                    window.LastTransactionSequence == 1,
                    "LENGTHEN native sets total at picked endpoint and preserves ID");
                window.Undo();
                Check(LineMatches(FindLine(window, sourceId), 0, 0, 10, 0),
                    "LENGTHEN native undo restores length");
                window.Redo();
                Check(FindLine(window, sourceId) == lengthened, "LENGTHEN native redo restores total");
            });

            RunEditCase(tempDirectory, "stretch", window =>
            {
                var sourceId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                window.SelectAt(new ArcCadPoint(5, 0));
                EnterCommand(window, "S");
                Check(window.ActiveDrawingTool == "Stretch", "S alias starts native STRETCH");
                window.AcceptPoint(new ArcCadPoint(9, -1));
                window.AcceptPoint(new ArcCadPoint(11, 1));
                window.AcceptPoint(new ArcCadPoint(10, 0));
                window.MovePointer(new ArcCadPoint(10, 5));
                Check(Named<CadViewport>(window, "WorkspaceViewport").PreviewVertices.Length == 20,
                    "STRETCH shows crossing window and displacement preview");
                window.AcceptPoint(new ArcCadPoint(10, 5));

                var stretched = FindLine(window, sourceId);
                Check(LineMatches(stretched, 0, 0, 10, 5) && window.Lines.Length == 1 &&
                    window.LastTransactionSequence == 1,
                    "STRETCH native moves only captured endpoint and preserves ID");
                window.Undo();
                Check(LineMatches(FindLine(window, sourceId), 0, 0, 10, 0),
                    "STRETCH native undo restores endpoint");
                window.Redo();
                Check(FindLine(window, sourceId) == stretched, "STRETCH native redo restores endpoint");
            });

            RunEditCase(tempDirectory, "join", window =>
            {
                var keepId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(3, 0));
                var removeId = CreateLine(window, new ArcCadPoint(5, 0), new ArcCadPoint(10, 0));
                window.SelectAt(new ArcCadPoint(1, 0));
                EnterCommand(window, "J");
                Check(window.ActiveDrawingTool == "Join", "J alias starts native JOIN");
                window.AcceptPoint(new ArcCadPoint(7, 0));

                var joined = FindLine(window, keepId);
                Check(LineMatches(joined, 0, 0, 10, 0) && window.Lines.Length == 1 &&
                    window.Lines.ToArray().All(line => line.EntityId != removeId) &&
                    window.LastTransactionSequence == 2,
                    "JOIN native keeps first ID and removes second in one transaction");
                window.Undo();
                Check(LineMatches(FindLine(window, keepId), 0, 0, 3, 0) &&
                    LineMatches(FindLine(window, removeId), 5, 0, 10, 0) && window.Lines.Length == 2,
                    "JOIN native undo restores both IDs");
                window.Redo();
                Check(window.Lines.Length == 1 && FindLine(window, keepId) == joined,
                    "JOIN native redo restores merged source ID");
            });
        }
        finally
        {
            Directory.Delete(tempDirectory, recursive: true);
        }
    }

    private static void RunAlignmentCleanupCommands()
    {
        var tempDirectory = Path.Combine(Path.GetTempPath(), $"ArcCAD-Alignment-Cleanup-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDirectory);
        try
        {
            RunEditCase(tempDirectory, "align", window =>
            {
                var sourceId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                window.SelectAt(new ArcCadPoint(5, 0));
                EnterCommand(window, "AL");
                Check(window.ActiveDrawingTool == "Align", "AL alias starts native ALIGN");
                window.AcceptPoint(new ArcCadPoint(0, 0));
                window.AcceptPoint(new ArcCadPoint(20, 20));
                window.AcceptPoint(new ArcCadPoint(10, 0));
                window.MovePointer(new ArcCadPoint(20, 30));
                Check(Named<CadViewport>(window, "WorkspaceViewport").PreviewVertices.Length == 8,
                    "ALIGN shows both source and destination directions");
                window.AcceptPoint(new ArcCadPoint(20, 30));

                var aligned = FindLine(window, sourceId);
                Check(LineMatches(aligned, 20, 20, 20, 30) && window.Lines.Length == 1 &&
                    window.LastTransactionSequence == 1,
                    "ALIGN native translates and rotates while preserving ID");
                window.Undo();
                Check(LineMatches(FindLine(window, sourceId), 0, 0, 10, 0),
                    "ALIGN native undo restores source");
                window.Redo();
                Check(FindLine(window, sourceId) == aligned, "ALIGN native redo restores alignment");
            });

            RunEditCase(tempDirectory, "nudge", window =>
            {
                var sourceId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                var source = FindLine(window, sourceId);
                window.SelectAt(new ArcCadPoint(5, 0));
                EnterCommand(window, "NUDGE 0 0");
                Check(FindLine(window, sourceId) == source && window.LastTransactionSequence == 0,
                    "NUDGE rejects zero vector without transaction");
                EnterCommand(window, "NUDGE 2 -3");

                var nudged = FindLine(window, sourceId);
                Check(LineMatches(nudged, 2, -3, 12, -3) && window.Lines.Length == 1 &&
                    window.LastTransactionSequence == 1,
                    "NUDGE native applies signed vector and preserves ID");
                window.Undo();
                Check(FindLine(window, sourceId) == source, "NUDGE native undo restores source");
                window.Redo();
                Check(FindLine(window, sourceId) == nudged, "NUDGE native redo restores vector");
            });

            RunEditCase(tempDirectory, "overkill", window =>
            {
                var keepId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                var removeId = CreateLine(window, new ArcCadPoint(0, 0), new ArcCadPoint(10, 0));
                var distinctId = CreateLine(window, new ArcCadPoint(0, 5), new ArcCadPoint(10, 5));
                window.SelectAt(new ArcCadPoint(5, 0));
                EnterCommand(window, "OVERKILL");

                Check(window.Lines.Length == 2 && FindLine(window, keepId).EntityId == keepId &&
                    FindLine(window, distinctId).EntityId == distinctId &&
                    window.Lines.ToArray().All(line => line.EntityId != removeId) &&
                    window.LastTransactionSequence == 3,
                    "OVERKILL native keeps oldest duplicate ID in one transaction");
                window.Undo();
                Check(window.Lines.Length == 3 && FindLine(window, removeId).EntityId == removeId,
                    "OVERKILL native undo restores duplicate ID");
                window.Redo();
                Check(window.Lines.Length == 2 && window.Lines.ToArray().All(line => line.EntityId != removeId),
                    "OVERKILL native redo removes duplicate again");

                window.SelectAt(new ArcCadPoint(5, 0));
                EnterCommand(window, "OVERKILL");
                Check(window.IsBackendConnected && window.Lines.Length == 2 &&
                    window.LastTransactionSequence == 3,
                    "OVERKILL no-op keeps backend, document and sequence intact");
                window.Undo();
                Check(window.Lines.Length == 3,
                    "OVERKILL no-op creates no visible undo group");
                window.Redo();
            });
        }
        finally
        {
            Directory.Delete(tempDirectory, recursive: true);
        }
    }

    private static void RunClosedShapeCommands()
    {
        var tempDirectory = Path.Combine(Path.GetTempPath(), $"ArcCAD-Closed-Shapes-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDirectory);
        try
        {
            RunEditCase(tempDirectory, "polygon-circumscribed", window =>
            {
                EnterCommand(window, "POLYGON 2 C");
                Check(!window.IsLineActive && window.EntityCount == 0 &&
                    window.LastTransactionSequence is null,
                    "POLYGON rejects side count outside 3..1024 without transaction");

                EnterCommand(window, "POLYGON 4 C");
                Check(window.ActiveDrawingTool == "Polygon", "circumscribed POLYGON command route");
                window.AcceptPoint(new ArcCadPoint(50, 50));
                window.MovePointer(new ArcCadPoint(70, 50));
                Check(Named<CadViewport>(window, "WorkspaceViewport").PreviewVertices.Length == 16,
                    "circumscribed square preview");
                window.AcceptPoint(new ArcCadPoint(70, 50));

                var polygonId = window.LastCreatedEntityId;
                var polygon = window.Entities.Span[0];
                Check(polygon.EntityId == polygonId && polygon.PointCount == 5 &&
                    window.Lines.Length == 0 && window.LastTransactionSequence == 0,
                    "circumscribed POLYGON creates one closed native entity");
                window.SelectAt(new ArcCadPoint(50 + 20 * Math.Sqrt(2), 50));
                var report = window.MeasureSelectedArea();
                Check(report.Contains("Area = 1600.0000", StringComparison.Ordinal) &&
                    report.Contains("Perimeter = 160.0000", StringComparison.Ordinal),
                    "circumscribed square uses apothem for exact AREA and perimeter");
                window.Undo();
                Check(window.EntityCount == 0, "circumscribed POLYGON undo");
                window.Redo();
                Check(window.Entities.Span[0].EntityId == polygonId &&
                    window.Entities.Span[0].Vertices.Span.SequenceEqual(polygon.Vertices.Span),
                    "circumscribed POLYGON redo preserves ID and geometry");
            });
        }
        finally
        {
            Directory.Delete(tempDirectory, recursive: true);
        }
    }

    private static void RunRectangleModifierCommands(string? evidenceDirectory)
    {
        var tempDirectory = Path.Combine(Path.GetTempPath(), $"ArcCAD-Rectangle-Modifiers-{Guid.NewGuid():N}");
        Directory.CreateDirectory(tempDirectory);
        try
        {
            RunEditCase(tempDirectory, "rectang-modifiers", window =>
            {
                var tree = ReadTree(window);
                EnterCommand(window, "RECTANG CHAMFER 2");
                Check(!window.IsLineActive && window.EntityCount == 0 &&
                    window.LastTransactionSequence is null &&
                    (tree.StatusText.Text ?? string.Empty).Contains("Parámetros inválidos", StringComparison.Ordinal),
                    "RECTANG rejects an incomplete CHAMFER without transaction");
                EnterCommand(window, "REC FILLET NaN");
                Check(!window.IsLineActive && window.EntityCount == 0 &&
                    window.LastTransactionSequence is null,
                    "RECTANG rejects a non-finite FILLET without transaction");
                EnterCommand(window, "RECTANGLE FILLET 2 CHAMFER 1 1");
                Check(!window.IsLineActive && window.EntityCount == 0 &&
                    window.LastTransactionSequence is null,
                    "RECTANG rejects mutually exclusive modifiers without transaction");

                EnterCommand(window, "RECTANGLE WIDTH 3");
                Check(window.ActiveDrawingTool == "Rectangle" &&
                    window.ActiveCommand == "RECTANG" &&
                    window.ActiveRectangleMode == "BASIC" &&
                    window.ActiveRectangleWidth == 3,
                    "RECTANGLE alias routes a width-only command through the formal session");
                window.AcceptPoint(new ArcCadPoint(10, 10));
                Check(window.HandleKey(Key.Escape, KeyModifiers.None) && !window.IsLineActive &&
                    window.EntityCount == 0 && window.LastTransactionSequence is null &&
                    window.ActiveRectangleWidth == 0,
                    "RECTANG cancel is non-mutating and resets modifier state");

                ExecuteLayerMutation(window, "LAYER NEW BK_RECT", "create RECTANG focal layer");
                var focalLayer = window.Layers.Single(layer => layer.Name == "BK_RECT");
                ExecuteLayerMutation(
                    window,
                    $"LAYER SET-CURRENT {focalLayer.Id}",
                    "set RECTANG focal layer current");
                Check(window.Layers.Single(layer => layer.Id == focalLayer.Id).Current,
                    "RECTANG focal layer is current before creation");

                var beforeChamferCount = window.EntityCount;
                var beforeChamferTransaction = window.LastTransactionSequence!.Value;
                EnterCommand(window, "REC WIDTH 2 CHAMFER 10 5");
                Check(window.ActiveDrawingTool == "Rectangle" &&
                    window.ActiveRectangleMode == "CHAMFER" &&
                    window.ActiveRectangleWidth == 2,
                    "REC parses CHAMFER and WIDTH in either option order");
                window.AcceptPoint(new ArcCadPoint(100, 100));
                window.MovePointer(new ArcCadPoint(200, 180));
                Check(tree.Viewport.PreviewVertices.Length == 32 &&
                    window.EntityCount == beforeChamferCount &&
                    window.LastTransactionSequence == beforeChamferTransaction,
                    "RECTANG CHAMFER preview is eight ephemeral segments");
                window.AcceptPoint(new ArcCadPoint(200, 180));
                var chamferId = window.LastCreatedEntityId;
                var chamfer = window.Entities.ToArray().Single(entity => entity.EntityId == chamferId);
                Check(window.EntityCount == beforeChamferCount + 1 &&
                    window.LastTransactionSequence == beforeChamferTransaction + 1 &&
                    chamfer.PointCount == 9 && chamfer.PolyWidth == 2 &&
                    !chamfer.IsLine && !window.IsLineActive,
                    "RECTANG CHAMFER creates one wide closed native polyline in one transaction");
                Dispatcher.UIThread.RunJobs();
                CheckColorNear(
                    CaptureFrame(window),
                    WorldPixel(tree, 150, 100),
                    Drawing,
                    2,
                    "RECTANG CHAMFER width renders visible pixels");
                window.SelectAt(new ArcCadPoint(150, 100));
                Dispatcher.UIThread.RunJobs();
                Check(window.SelectedEntityId == chamferId && tree.PropertyType.Text == "LWPOLYLINE",
                    "RECTANG CHAMFER supports native hit and type");
                var chamferArea = window.MeasureSelectedArea();
                Check(chamferArea.Contains("Area = 7900.0000", StringComparison.Ordinal) &&
                    chamferArea.Contains("Perimeter = 344.7214", StringComparison.Ordinal),
                    $"RECTANG CHAMFER reports exact centerline AREA; actual={chamferArea}");
                var chamferBounds = ParseMeasuredBounds(window.MeasureSelectedBounds());
                Check(Math.Abs(chamferBounds.MinX - 100) < 0.0001 &&
                    Math.Abs(chamferBounds.MinY - 100) < 0.0001 &&
                    Math.Abs(chamferBounds.MaxX - 200) < 0.0001 &&
                    Math.Abs(chamferBounds.MaxY - 180) < 0.0001,
                    "RECTANG CHAMFER has exact native centerline extents");
                window.ListSelectedEntity();
                Check((tree.StatusText.Text ?? string.Empty).Contains("Layer: BK_RECT", StringComparison.Ordinal),
                    "RECTANG CHAMFER uses the current layer");
                window.Undo();
                Check(window.Entities.ToArray().All(entity => entity.EntityId != chamferId),
                    "RECTANG CHAMFER undo removes one entity");
                window.Redo();
                Check(window.Entities.ToArray().Single(entity => entity.EntityId == chamferId)
                        .Vertices.Span.SequenceEqual(chamfer.Vertices.Span),
                    "RECTANG CHAMFER redo restores exact ID and geometry");

                var beforeFilletCount = window.EntityCount;
                var beforeFilletTransaction = window.LastTransactionSequence!.Value;
                EnterCommand(window, "RECTANG FILLET 10 WIDTH 4");
                Check(window.ActiveRectangleMode == "FILLET" && window.ActiveRectangleWidth == 4,
                    "RECTANG parses FILLET with WIDTH");
                window.AcceptPoint(new ArcCadPoint(300, 100));
                window.MovePointer(new ArcCadPoint(400, 180));
                Check(tree.Viewport.PreviewVertices.Length == 112 &&
                    window.EntityCount == beforeFilletCount &&
                    window.LastTransactionSequence == beforeFilletTransaction,
                    "RECTANG FILLET preview has four lines and four tessellated quarter arcs");
                window.AcceptPoint(new ArcCadPoint(400, 180));
                var filletId = window.LastCreatedEntityId;
                var fillet = window.Entities.ToArray().Single(entity => entity.EntityId == filletId);
                Check(window.EntityCount == beforeFilletCount + 1 &&
                    window.LastTransactionSequence == beforeFilletTransaction + 1 &&
                    fillet.PointCount > 9 && fillet.PolyWidth == 4 &&
                    fillet.AnalyticLength is { } filletLength &&
                    Math.Abs(filletLength - 342.8318530717959) < 0.000001,
                    "RECTANG FILLET creates one exact bulge polyline with analytic length");
                Dispatcher.UIThread.RunJobs();
                CheckColorNear(
                    CaptureFrame(window),
                    WorldPixel(tree, 350, 100),
                    Drawing,
                    3,
                    "RECTANG FILLET width renders visible pixels");
                window.SelectAt(new ArcCadPoint(350, 100));
                var filletArea = window.MeasureSelectedArea();
                Check(window.SelectedEntityId == filletId && tree.PropertyType.Text == "LWPOLYLINE" &&
                    filletArea.Contains("Area = 7914.1593", StringComparison.Ordinal) &&
                    filletArea.Contains("Perimeter = 342.8319", StringComparison.Ordinal),
                    $"RECTANG FILLET supports hit and exact bulge AREA; actual={filletArea}");
                var filletBounds = ParseMeasuredBounds(window.MeasureSelectedBounds());
                Check(Math.Abs(filletBounds.MinX - 300) < 0.0001 &&
                    Math.Abs(filletBounds.MinY - 100) < 0.0001 &&
                    Math.Abs(filletBounds.MaxX - 400) < 0.0001 &&
                    Math.Abs(filletBounds.MaxY - 180) < 0.0001,
                    "RECTANG FILLET has exact native centerline extents");
                window.Undo();
                Check(window.Entities.ToArray().All(entity => entity.EntityId != filletId),
                    "RECTANG FILLET undo removes one entity");
                window.Redo();
                Check(window.Entities.ToArray().Single(entity => entity.EntityId == filletId)
                        .Vertices.Span.SequenceEqual(fillet.Vertices.Span),
                    "RECTANG FILLET redo restores exact ID and geometry");

                var beforeOverlapCount = window.EntityCount;
                var beforeOverlapTransaction = window.LastTransactionSequence!.Value;
                EnterCommand(window, "REC FILLET 40");
                window.AcceptPoint(new ArcCadPoint(500, 100));
                ExpectThrows<ArgumentException>(
                    () => window.AcceptPoint(new ArcCadPoint(600, 180)),
                    "RECTANG rejects a FILLET that collapses straight sides");
                Check(window.IsLineActive && window.EntityCount == beforeOverlapCount &&
                    window.LastTransactionSequence == beforeOverlapTransaction,
                    "RECTANG FILLET overlap rejection is atomic and recoverable");
                window.MovePointer(new ArcCadPoint(600, 200));
                window.AcceptPoint(new ArcCadPoint(600, 200));
                var recoveredId = window.LastCreatedEntityId;
                Check(!window.IsLineActive && window.EntityCount == beforeOverlapCount + 1 &&
                    window.LastTransactionSequence == beforeOverlapTransaction + 1,
                    "RECTANG recovers in the same command after a rejected second corner");

                if (evidenceDirectory is not null)
                {
                    window.SelectAt(new ArcCadPoint(350, 100));
                    Dispatcher.UIThread.RunJobs();
                    File.WriteAllBytes(
                        Path.Combine(evidenceDirectory, "rectangle-modifiers.png"),
                        CaptureFrame(window).Png);
                }

                var reopenPath = Path.Combine(tempDirectory, "rectang-modifiers-reopen.arcf");
                window.SaveToPath(reopenPath);
                window.NewDocument();
                var warnings = window.OpenFromPath(reopenPath);
                var reopenedChamfer = window.Entities.ToArray()
                    .Single(entity => entity.EntityId == chamferId);
                var reopenedFillet = window.Entities.ToArray()
                    .Single(entity => entity.EntityId == filletId);
                Check(warnings.Count == 0 && window.LastTransactionSequence is null &&
                    reopenedChamfer.Vertices.Span.SequenceEqual(chamfer.Vertices.Span) &&
                    reopenedChamfer.PolyWidth == 2 &&
                    reopenedFillet.Vertices.Span.SequenceEqual(fillet.Vertices.Span) &&
                    reopenedFillet.PolyWidth == 4 &&
                    window.Entities.ToArray().Any(entity => entity.EntityId == recoveredId),
                    "RECTANG modifiers reopen with exact IDs, geometry, bulges and width");
                window.SelectAt(new ArcCadPoint(350, 100));
                var reopenedArea = window.MeasureSelectedArea();
                Check(window.SelectedEntityId == filletId &&
                    reopenedArea.Contains("Area = 7914.1593", StringComparison.Ordinal) &&
                    !window.CanUndo && !window.CanRedo,
                    "reopened RECTANG FILLET keeps exact AREA without history");
            });
        }
        finally
        {
            Directory.Delete(tempDirectory, recursive: true);
        }
    }

    private static void RunEditCase(string directory, string name, Action<MainWindow> check)
    {
        var path = Path.Combine(directory, $"{name}.arcf");
        MainWindow? window = null;
        try
        {
            window = new MainWindow { Width = 1280, Height = 720 };
            window.Show();
            Dispatcher.UIThread.RunJobs();
            Click(Named<Button>(window, "OsnapStatusButton"));
            Check(!window.IsObjectSnapEnabled, $"{name} exact test disables OSNAP");
            check(window);
            window.SaveToPath(path);
            window.Close();
            Dispatcher.UIThread.RunJobs();
            Check(!window.IsBackendConnected, $"{name} native edit window disposed");
        }
        finally
        {
            if (window?.IsBackendConnected == true)
            {
                if (window.IsDirty)
                {
                    window.SaveToPath(path);
                }

                window.Close();
                Dispatcher.UIThread.RunJobs();
            }
        }
    }

    private static ulong CreateLine(MainWindow window, ArcCadPoint first, ArcCadPoint second)
    {
        window.StartLine();
        window.AcceptPoint(first);
        window.AcceptPoint(second);
        window.CancelLine();
        return window.LineEntityId;
    }

    private static void EnterCommand(MainWindow window, string command)
    {
        var input = Named<TextBox>(window, "CommandInput");
        input.Text = command;
        input.Focus();
        window.KeyPressQwerty(PhysicalKey.Enter, RawInputModifiers.None);
        Dispatcher.UIThread.RunJobs();
    }

    private static CadLine FindLine(MainWindow window, ulong entityId) =>
        window.Lines.ToArray().Single(line => line.EntityId == entityId);

    private static bool LineMatches(CadLine line, double x1, double y1, double x2, double y2) =>
        Near(line.X1, x1) && Near(line.Y1, y1) && Near(line.X2, x2) && Near(line.Y2, y2) ||
        Near(line.X1, x2) && Near(line.Y1, y2) && Near(line.X2, x1) && Near(line.Y2, y1);

    private static bool Near(double left, double right) => Math.Abs(left - right) <= 0.000000001;

    private static (double MinX, double MinY, double MaxX, double MaxY) ParseMeasuredBounds(
        string value)
    {
        var match = Regex.Match(
            value,
            @"Min = (?<minX>[-+]?\d+(?:\.\d+)?),(?<minY>[-+]?\d+(?:\.\d+)?).*" +
            @"Max = (?<maxX>[-+]?\d+(?:\.\d+)?),(?<maxY>[-+]?\d+(?:\.\d+)?)",
            RegexOptions.CultureInvariant);
        Check(match.Success, $"native bounds format; actual={value}");
        return (
            double.Parse(match.Groups["minX"].Value, CultureInfo.InvariantCulture),
            double.Parse(match.Groups["minY"].Value, CultureInfo.InvariantCulture),
            double.Parse(match.Groups["maxX"].Value, CultureInfo.InvariantCulture),
            double.Parse(match.Groups["maxY"].Value, CultureInfo.InvariantCulture));
    }

    private static void Commit(MainWindow window, ArcCadPoint point, ulong expectedSequence)
    {
        window.MovePointer(point);
        window.AcceptPoint(point);
        Dispatcher.UIThread.RunJobs();
        Check(window.LastTransactionSequence == expectedSequence, $"LINE txSeq {expectedSequence}");
        Check(window.Lines.Length == checked((int)expectedSequence + 1), $"LINE count after txSeq {expectedSequence}");
    }

    private static ExistingUiTree ReadTree(MainWindow window)
    {
        var root = Named<Grid>(window, "RootOverlayGrid");
        var generalProperties = Named<StackPanel>(window, "GeneralPropertyRows");
        var unavailable = generalProperties.GetLogicalDescendants()
            .OfType<TextBlock>()
            .Single(control => control.Text == "No disponible");
        return new ExistingUiTree(
            window,
            root,
            Named<Border>(window, "ApplicationChrome"),
            Named<Border>(window, "TitleBar"),
            Named<Grid>(window, "TitleLayout"),
            Named<StackPanel>(window, "QuickAccessToolbar"),
            Named<Grid>(window, "RibbonInicioPanel"),
            Named<Button>(window, "HomeRibbonTab"),
            Named<Button>(window, "ToolsRibbonTab"),
            Named<ItemsControl>(window, "RibbonContextPanel"),
            Named<Grid>(window, "WorkspaceGrid"),
            Named<CadViewport>(window, "WorkspaceViewport"),
            Named<Border>(window, "PropertiesDock"),
            Named<StackPanel>(window, "PropertiesPanel"),
            Named<ComboBox>(window, "PropertiesSelectionCombo"),
            Named<TextBlock>(window, "PropertyTypeText"),
            Named<TextBlock>(window, "PropertyIdText"),
            Named<TextBlock>(window, "PropertyLengthText"),
            unavailable,
            Named<TextBlock>(window, "BackendStatusText"),
            Named<TextBlock>(window, "DocumentTitleText"),
            Named<Grid>(window, "CommandBar"),
            Named<TextBox>(window, "CommandInput"),
            Named<TextBox>(window, "GlobalSearchBox"),
            Named<StackPanel>(window, "LayoutTabsPanel"),
            Named<Button>(window, "ModelLayoutButton"),
            Named<Button>(window, "GridStatusButton"),
            Named<Button>(window, "UcsButton"),
            Named<Button>(window, "OsnapStatusButton"),
            Named<Button>(window, "OrthoStatusButton"),
            Named<Button>(window, "ViewportMenuButton"),
            Named<Button>(window, "PropertiesRibbonButton"),
            Named<Button>(window, "ClosePropertiesButton"),
            Named<Button>(window, "LayerManagerButton"),
            Named<Button>(window, "CloseLayersButton"),
            Named<Border>(window, "LayerManagerPanel"),
            Named<Button>(window, "NewButton"),
            Named<Button>(window, "OpenButton"),
            Named<Button>(window, "SaveButton"),
            Named<Button>(window, "LineButton"),
            Named<Button>(window, "RailLineButton"),
            Named<Button>(window, "UndoButton"),
            Named<Button>(window, "RedoButton"),
            Named<Button>(window, "NewDocumentButton"),
            Named<Button>(window, "NewWorkspaceDocumentButton"),
            Named<Button>(window, "SelectToolButton"),
            Named<Button>(window, "HomePolylineButton"),
            Named<Button>(window, "HomeRectangleButton"),
            Named<Button>(window, "HomeCircleButton"),
            Named<Button>(window, "HomeArcButton"),
            Named<Button>(window, "HomeMultilineButton"),
            Named<Button>(window, "HomeMoveButton"),
            Named<Button>(window, "HomeRotateButton"),
            Named<Button>(window, "HomeCopyButton"),
            Named<Button>(window, "HomeMirrorButton"),
            Named<Button>(window, "HomeArrayButton"),
            Named<Button>(window, "HomeDimensionButton"),
            Named<Button>(window, "LineweightStatusButton"),
            Named<Button>(window, "AnnotationStatusButton"),
            Named<Button>(window, "ZoomInButton"),
            Named<Button>(window, "PanButton"),
            Named<Button>(window, "FitViewButton"),
            Named<Button>(window, "ResetViewButton"),
            Named<Button>(window, "LayerRowsToggleButton"),
            Named<Grid>(window, "LayerRowsPanel"),
            Named<Button>(window, "GeneralSectionButton"),
            generalProperties,
            Named<Button>(window, "ViewSectionButton"),
            Named<StackPanel>(window, "ViewPropertyRows"),
            Named<Button>(window, "MiscSectionButton"),
            Named<StackPanel>(window, "MiscPropertyRows"),
            Named<Button>(window, "HomeTextButton"));
    }

    private static void CheckLayout(MainWindow window, ExistingUiTree tree, Size expected)
    {
        Check(window.ClientSize == expected, $"{expected.Width}x{expected.Height} client size");
        var frame = new Rect(window.ClientSize);
        foreach (var region in new Control[]
        {
            tree.Root,
            tree.ApplicationChrome,
            tree.TitleBar,
            tree.QuickAccessToolbar,
            tree.Ribbon,
            tree.Workspace,
            tree.Viewport,
            tree.PropertiesDock,
            tree.CommandBar,
            tree.LayoutTabs,
        })
        {
            var bounds = BoundsIn(region, window);
            Check(region.IsVisible && bounds.Width > 0 && bounds.Height > 0, $"{region.Name} visible bounds");
            Check(Contains(frame, bounds), $"{region.Name} frame containment");
        }

        var title = BoundsIn(tree.TitleBar, window);
        var ribbon = BoundsIn(tree.Ribbon, window);
        var workspace = BoundsIn(tree.Workspace, window);
        var viewport = BoundsIn(tree.Viewport, window);
        var properties = BoundsIn(tree.PropertiesDock, window);
        var command = BoundsIn(tree.CommandBar, window);
        var layouts = BoundsIn(tree.LayoutTabs, window);
        var qat = BoundsIn(tree.QuickAccessToolbar, window);
        var railLine = BoundsIn(tree.RailLineButton, window);

        Check(title.Bottom <= ribbon.Top, "title/ribbon order");
        Check(ribbon.Bottom <= workspace.Top, "ribbon/workspace order");
        Check(workspace.Bottom <= layouts.Top, "workspace/layout order");
        Check(Contains(workspace, viewport) && Contains(workspace, properties), "workspace region containment");
        Check(IntersectionArea(viewport, properties) == 0 && viewport.Right <= properties.Left,
            "viewport/properties separation");
        Check(Contains(viewport, command), "command bar in viewport");
        Check(qat.Top >= title.Bottom && qat.Bottom <= ribbon.Top, "QAT row placement");
        Check(railLine.Right <= viewport.Left, "rail/viewport order");
        Check(viewport.Width >= 700 && viewport.Height >= 300, "usable viewport");
        Check(tree.ApplicationChrome.CornerRadius.TopLeft == 4, "restrained chrome radius");
        Check(tree.ModelLayout.IsVisible && tree.GridStatus.IsVisible, "layout/status regions");
    }

    private static void CheckSingleBackend(MainWindow window, ExistingUiTree tree)
    {
        var sessionFields = typeof(MainWindow)
            .GetFields(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic)
            .Where(field => typeof(WorkspaceSession).IsAssignableFrom(field.FieldType))
            .ToArray();
        Check(sessionFields.Length == 1 && sessionFields[0].GetValue(window) is WorkspaceSession,
            "exactly one WorkspaceSession");
        Check(window.GetLogicalDescendants().OfType<CadViewport>().Count() == 1 &&
            ReferenceEquals(window.GetLogicalDescendants().OfType<CadViewport>().Single(), tree.Viewport),
            "exactly one real viewport");
    }

    private static UiTree ReadLegacyTree(MainWindow window)
    {
        var shell = Exact<Grid>(window.Content, "ShellRoot type");
        Check(shell.Name == "ShellRoot" && shell.Children.Count == 4, "ShellRoot structure");
        var product = Exact<Border>(shell.Children[0], "ProductRegion type");
        var ribbon = Exact<Border>(shell.Children[1], "RibbonRegion type");
        var workspace = Exact<Grid>(shell.Children[2], "WorkspaceContent type");
        var status = Exact<Border>(shell.Children[3], "StatusRegion type");
        Check(
            product.Name == "ProductRegion" && ribbon.Name == "RibbonRegion" &&
            workspace.Name == "WorkspaceContent" && status.Name == "StatusRegion",
            "region names");

        var productContent = Exact<Grid>(product.Child, "ProductContent type");
        Check(productContent.Name == "ProductContent" && productContent.Children.Count == 3, "ProductContent structure");
        var heading = Exact<TextBlock>(productContent.Children[0], "ProductHeading type");
        var productDescriptor = Exact<TextBlock>(productContent.Children[1], "product descriptor type");
        var documentLabel = Exact<TextBlock>(productContent.Children[2], "document label type");
        Check(
            heading.Name == "ProductHeading" && heading.Text == "ArcCAD" &&
            productDescriptor.Text == "Dibujo 2D" && documentLabel.Text == "Documento ArcCAD · .arcf",
            "header content");

        var toolbar = Exact<Grid>(ribbon.Child, "Toolbar type");
        Check(toolbar.Name == "Toolbar" && toolbar.Children.Count == 3, "toolbar structure");
        var fileGroup = Exact<Border>(toolbar.Children[0], "FileGroup type");
        var drawGroup = Exact<Border>(toolbar.Children[1], "DrawGroup type");
        var historyGroup = Exact<Border>(toolbar.Children[2], "HistoryGroup type");
        Check(
            fileGroup.Name == "FileGroup" && drawGroup.Name == "DrawGroup" && historyGroup.Name == "HistoryGroup",
            "ribbon group names");

        var fileContent = Exact<Grid>(fileGroup.Child, "FileGroup content type");
        Check(fileContent.Children.Count == 2, "FileGroup content count");
        var fileLabel = Exact<TextBlock>(fileContent.Children[0], "FileGroup label type");
        var fileButtons = Exact<StackPanel>(fileContent.Children[1], "FileGroup buttons type");
        Check(fileLabel.Text == "ARCHIVO" && fileButtons.Children.Count == 3, "FileGroup structure");
        var newButton = Exact<Button>(fileButtons.Children[0], "NewButton type");
        var openButton = Exact<Button>(fileButtons.Children[1], "OpenButton type");
        var saveButton = Exact<Button>(fileButtons.Children[2], "SaveButton type");

        var drawContent = Exact<Grid>(drawGroup.Child, "DrawGroup content type");
        Check(drawContent.Children.Count == 2, "DrawGroup content count");
        var drawLabel = Exact<TextBlock>(drawContent.Children[0], "DrawGroup label type");
        var lineButton = Exact<Button>(drawContent.Children[1], "LineButton type");
        Check(drawLabel.Text == "DIBUJO", "DrawGroup structure");

        var historyContent = Exact<Grid>(historyGroup.Child, "HistoryGroup content type");
        Check(historyContent.Children.Count == 2, "HistoryGroup content count");
        var historyLabel = Exact<TextBlock>(historyContent.Children[0], "HistoryGroup label type");
        var historyButtons = Exact<StackPanel>(historyContent.Children[1], "HistoryGroup buttons type");
        Check(historyLabel.Text == "HISTORIAL" && historyButtons.Children.Count == 2, "HistoryGroup structure");
        var undoButton = Exact<Button>(historyButtons.Children[0], "UndoButton type");
        var redoButton = Exact<Button>(historyButtons.Children[1], "RedoButton type");
        Check(newButton.Name == "NewButton" && openButton.Name == "OpenButton" &&
            saveButton.Name == "SaveButton" && lineButton.Name == "LineButton" &&
            undoButton.Name == "UndoButton" && redoButton.Name == "RedoButton", "button names");

        Check(workspace.Children.Count == 2, "WorkspaceContent child count");
        var viewport = Exact<CadViewport>(workspace.Children[0], "WorkspaceViewport type");
        var inspector = Exact<Border>(workspace.Children[1], "InspectorRegion type");
        Check(viewport.Name == "WorkspaceViewport" && inspector.Name == "InspectorRegion", "workspace child names");
        var inspectorContent = Exact<Grid>(inspector.Child, "inspector content type");
        Check(inspectorContent.Children.Count == 4, "inspector content count");
        var propertiesHeading = Exact<TextBlock>(inspectorContent.Children[0], "PropertiesHeading type");
        var selectionContext = Exact<TextBlock>(inspectorContent.Children[1], "selection context type");
        var selectionHelp = Exact<TextBlock>(inspectorContent.Children[2], "selection help type");
        var properties = Exact<Border>(inspectorContent.Children[3], "PropertiesPanel type");
        Check(propertiesHeading.Name == "PropertiesHeading" && properties.Name == "PropertiesPanel", "property regions");
        var propertiesContent = Exact<StackPanel>(properties.Child, "properties content type");
        Check(propertiesContent.Name == "PropertiesContent" && propertiesContent.Children.Count == 3, "properties content count");
        var propertyType = Exact<TextBlock>(propertiesContent.Children[0], "PropertyTypeText type");
        var propertyId = Exact<TextBlock>(propertiesContent.Children[1], "PropertyIdText type");
        var propertyLength = Exact<TextBlock>(propertiesContent.Children[2], "PropertyLengthText type");
        Check(propertyType.Name == "PropertyTypeText" && propertyId.Name == "PropertyIdText" &&
            propertyLength.Name == "PropertyLengthText", "property names");

        var statusContent = Exact<Grid>(status.Child, "StatusContent type");
        Check(statusContent.Name == "StatusContent" && statusContent.Children.Count == 2, "StatusContent structure");
        var commandRow = Exact<Grid>(statusContent.Children[0], "command row type");
        var statusLine = Exact<Grid>(statusContent.Children[1], "status line type");
        Check(commandRow.Children.Count == 2 && statusLine.Children.Count == 5, "status row structure");
        var commandLabel = Exact<TextBlock>(commandRow.Children[0], "command label type");
        var statusText = Exact<TextBlock>(commandRow.Children[1], "BackendStatusText type");
        var modelStatus = Exact<TextBlock>(statusLine.Children[0], "model status type");
        var gridStatus = Exact<TextBlock>(statusLine.Children[1], "grid status type");
        var ucsStatus = Exact<TextBlock>(statusLine.Children[2], "UCS status type");
        var snapStatus = Exact<TextBlock>(statusLine.Children[3], "snap status type");
        var shortcutStatus = Exact<TextBlock>(statusLine.Children[4], "shortcut status type");
        Check(
            commandLabel.Text == "COMANDO" && statusText.Name == "BackendStatusText" &&
            modelStatus.Text == "MODELO" && gridStatus.Text == "GRID" && ucsStatus.Text == "UCS" &&
            snapStatus.Text == "SNAP · Extremo" &&
            shortcutStatus.Text == "Esc cancelar · Ctrl+Z deshacer · Ctrl+Y rehacer",
            "status content");
        return new UiTree(
            shell, product, productContent, heading, productDescriptor, documentLabel,
            ribbon, toolbar,
            fileGroup, fileContent, fileLabel, fileButtons,
            drawGroup, drawContent, drawLabel,
            historyGroup, historyContent, historyLabel, historyButtons,
            newButton, openButton, saveButton, lineButton, undoButton, redoButton,
            workspace, viewport, inspector, inspectorContent, propertiesHeading, selectionContext, selectionHelp,
            properties, propertiesContent, propertyType, propertyId, propertyLength,
            status, statusContent, commandRow, commandLabel, statusText, statusLine,
            modelStatus, gridStatus, ucsStatus, snapStatus, shortcutStatus);
    }

    private static void CheckLegacyLayout(UiTree tree)
    {
        var shellArea = new Rect(0, 0, tree.Shell.Bounds.Width, tree.Shell.Bounds.Height);
        foreach (var region in new Control[] { tree.Product, tree.Ribbon, tree.Workspace, tree.Status })
        {
            Check(region.Bounds.Width > 0 && region.Bounds.Height > 0, $"{region.Name} bounds");
            Check(Contains(shellArea, region.Bounds), $"{region.Name} containment");
            Check(AtLeastDesired(region), $"{region.Name} desired size");
        }

        Check(IntersectionArea(tree.Product.Bounds, tree.Ribbon.Bounds) == 0, "product/ribbon overlap");
        Check(IntersectionArea(tree.Ribbon.Bounds, tree.Workspace.Bounds) == 0, "ribbon/workspace overlap");
        Check(IntersectionArea(tree.Workspace.Bounds, tree.Status.Bounds) == 0, "workspace/status overlap");
        Check(
            tree.Product.Bounds.Bottom <= tree.Ribbon.Bounds.Top &&
            tree.Ribbon.Bounds.Bottom <= tree.Workspace.Bounds.Top &&
            tree.Workspace.Bounds.Bottom <= tree.Status.Bounds.Top,
            "region order");
        Check(tree.Product.Bounds.Height + tree.Ribbon.Bounds.Height <= 160, "compact header and ribbon");
        Check(tree.Status.Bounds.Height <= 88, "compact command and status");

        Check(Contains(new Rect(tree.Product.Bounds.Size), tree.ProductContent.Bounds), "ProductContent containment");
        Check(Contains(new Rect(tree.ProductContent.Bounds.Size), tree.Heading.Bounds), "ProductHeading containment");
        Check(Contains(new Rect(tree.ProductContent.Bounds.Size), tree.ProductDescriptor.Bounds), "product descriptor containment");
        Check(Contains(new Rect(tree.ProductContent.Bounds.Size), tree.DocumentLabel.Bounds), "document label containment");
        Check(IntersectionArea(tree.Heading.Bounds, tree.ProductDescriptor.Bounds) == 0, "heading/descriptor overlap");
        Check(IntersectionArea(tree.ProductDescriptor.Bounds, tree.DocumentLabel.Bounds) == 0, "descriptor/document overlap");

        Check(Contains(new Rect(tree.Ribbon.Bounds.Size), tree.Toolbar.Bounds), "Toolbar containment");
        var toolbarArea = new Rect(tree.Toolbar.Bounds.Size);
        foreach (var group in new[] { tree.FileGroup, tree.DrawGroup, tree.HistoryGroup })
        {
            Check(group.IsVisible && group.Bounds.Width > 0 && group.Bounds.Height > 0, $"{group.Name} bounds");
            Check(Contains(toolbarArea, group.Bounds), $"{group.Name} containment");
        }

        Check(IntersectionArea(tree.FileGroup.Bounds, tree.DrawGroup.Bounds) == 0, "file/draw overlap");
        Check(IntersectionArea(tree.DrawGroup.Bounds, tree.HistoryGroup.Bounds) == 0, "draw/history overlap");
        Check(tree.FileGroup.Bounds.Right <= tree.DrawGroup.Bounds.Left &&
            tree.DrawGroup.Bounds.Right <= tree.HistoryGroup.Bounds.Left, "ribbon group order");
        Check(Contains(new Rect(tree.FileGroup.Bounds.Size), tree.FileContent.Bounds), "FileGroup content containment");
        Check(Contains(new Rect(tree.DrawGroup.Bounds.Size), tree.DrawContent.Bounds), "DrawGroup content containment");
        Check(Contains(new Rect(tree.HistoryGroup.Bounds.Size), tree.HistoryContent.Bounds), "HistoryGroup content containment");

        foreach (var button in new[]
        {
            tree.NewButton, tree.OpenButton, tree.SaveButton,
            tree.LineButton, tree.UndoButton, tree.RedoButton,
        })
        {
            Check(
                button.Bounds.Width > 0 && button.Bounds.Height >= 36 && button.MinHeight >= 36 && AtLeastDesired(button),
                $"{button.Name} layout");
        }

        var fileButtonsArea = new Rect(tree.FileButtons.Bounds.Size);
        Check(Contains(fileButtonsArea, tree.NewButton.Bounds), "NewButton containment");
        Check(Contains(fileButtonsArea, tree.OpenButton.Bounds), "OpenButton containment");
        Check(Contains(fileButtonsArea, tree.SaveButton.Bounds), "SaveButton containment");
        Check(Contains(new Rect(tree.DrawContent.Bounds.Size), tree.LineButton.Bounds), "LineButton containment");
        var historyButtonsArea = new Rect(tree.HistoryButtons.Bounds.Size);
        Check(Contains(historyButtonsArea, tree.UndoButton.Bounds), "UndoButton containment");
        Check(Contains(historyButtonsArea, tree.RedoButton.Bounds), "RedoButton containment");
        Check(IntersectionArea(tree.NewButton.Bounds, tree.OpenButton.Bounds) == 0, "new/open overlap");
        Check(IntersectionArea(tree.OpenButton.Bounds, tree.SaveButton.Bounds) == 0, "open/save overlap");
        Check(IntersectionArea(tree.UndoButton.Bounds, tree.RedoButton.Bounds) == 0, "undo/redo overlap");

        var workspaceArea = new Rect(tree.Workspace.Bounds.Size);
        Check(tree.Viewport.Bounds.Width > 0 && tree.Viewport.Bounds.Height > 0, "viewport bounds");
        Check(Contains(workspaceArea, tree.Viewport.Bounds), "viewport containment");
        Check(tree.Inspector.IsVisible && tree.Inspector.Bounds.Width is >= 260 and <= 304, "inspector visible width");
        Check(tree.Inspector.Bounds.Height > 0 && Contains(workspaceArea, tree.Inspector.Bounds), "inspector containment");
        Check(IntersectionArea(tree.Viewport.Bounds, tree.Inspector.Bounds) == 0, "viewport/inspector overlap");
        Check(tree.Viewport.Bounds.Right <= tree.Inspector.Bounds.Left, "viewport/inspector order");
        Check(tree.Viewport.Bounds.Width / tree.Workspace.Bounds.Width >= 0.68, "viewport width dominance");
        Check(
            tree.Viewport.Bounds.Width * tree.Viewport.Bounds.Height /
                (tree.Workspace.Bounds.Width * tree.Workspace.Bounds.Height) >= 0.65,
            "viewport area dominance");
        Check(Contains(new Rect(tree.Inspector.Bounds.Size), tree.InspectorContent.Bounds), "inspector content containment");
        var inspectorArea = new Rect(tree.InspectorContent.Bounds.Size);
        Check(Contains(inspectorArea, tree.PropertiesHeading.Bounds), "properties heading containment");
        Check(Contains(inspectorArea, tree.SelectionContext.Bounds), "selection context containment");
        Check(Contains(inspectorArea, tree.SelectionHelp.Bounds), "selection help containment");
        if (tree.Properties.IsVisible)
        {
            Check(tree.Properties.Bounds.Width > 0 && tree.Properties.Bounds.Height > 0, "properties bounds");
            Check(Contains(inspectorArea, tree.Properties.Bounds), "properties containment");
            Check(AtLeastDesired(tree.PropertiesContent), "properties desired size");
        }

        Check(Contains(new Rect(tree.Status.Bounds.Size), tree.StatusContent.Bounds), "status content containment");
        var statusArea = new Rect(tree.StatusContent.Bounds.Size);
        Check(Contains(statusArea, tree.CommandRow.Bounds), "command row containment");
        Check(Contains(statusArea, tree.StatusLine.Bounds), "status line containment");
        Check(IntersectionArea(tree.CommandRow.Bounds, tree.StatusLine.Bounds) == 0, "command/status overlap");
        Check(tree.CommandRow.Bounds.Bottom <= tree.StatusLine.Bounds.Top, "command/status order");
        Check(Contains(new Rect(tree.CommandRow.Bounds.Size), tree.CommandLabel.Bounds), "command label containment");
        Check(Contains(new Rect(tree.CommandRow.Bounds.Size), tree.StatusText.Bounds), "status text containment");
        var statusLineArea = new Rect(tree.StatusLine.Bounds.Size);
        foreach (var item in new[]
        {
            tree.ModelStatus, tree.GridStatus, tree.UcsStatus, tree.SnapStatus, tree.ShortcutStatus,
        })
        {
            Check(Contains(statusLineArea, item.Bounds) && AtLeastDesired(item), $"{item.Text} layout");
        }
    }

    private static void CheckButton(Button button, string content, string accessibleName, bool enabled)
    {
        Check(button.IsVisible && button.Focusable, $"{content} visibility/focusability");
        Check(button.Content is string value && value == content, $"{content} content");
        Check(AutomationProperties.GetName(button) == accessibleName, $"{content} accessible name");
        Check(button.IsEnabled == enabled, $"{content} enabled state");
    }

    private static void CheckActionButton(Button button, string tag, string accessibleName, bool enabled)
    {
        Check(button.IsVisible && button.Focusable, $"{tag} visibility/focusability");
        Check(button.Tag?.ToString() == tag, $"{tag} action tag");
        Check(AutomationProperties.GetName(button) == accessibleName, $"{tag} accessible name");
        Check(button.IsEnabled == enabled, $"{tag} enabled state");
    }

    private static T Named<T>(Control root, string name)
        where T : Control
    {
        var matches = root.GetLogicalDescendants()
            .Prepend(root)
            .OfType<T>()
            .Where(control => control.Name == name)
            .ToArray();
        Check(matches.Length == 1, $"single {name} anchor");
        return matches[0];
    }

    private static Rect BoundsIn(Control control, Visual relativeTo)
    {
        var origin = control.TranslatePoint(default, relativeTo);
        Check(origin is not null, $"{control.Name} translated bounds");
        return new Rect(origin!.Value, control.Bounds.Size);
    }

    private static void Click(Button button)
    {
        button.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
        Dispatcher.UIThread.RunJobs();
    }

    private static Button ActionButton(Control root, string tag)
    {
        var matches = root.GetVisualDescendants()
            .OfType<Button>()
            .Where(button => string.Equals(button.Tag?.ToString(), tag, StringComparison.Ordinal))
            .ToArray();
        Check(matches.Length == 1, $"single action button {tag} (found {matches.Length})");
        return matches[0];
    }

    private static T Exact<T>(object? value, string message)
        where T : class
    {
        Check(value?.GetType() == typeof(T), message);
        return (T)value!;
    }

    private static Color Resource(Window window, string key, Color expected)
    {
        Check(window.Resources.TryGetResource(key, window.ActualThemeVariant, out var value), $"missing resource {key}");
        var color = BrushColor(value, key);
        Check(color == expected, $"resource {key}");
        return color;
    }

    private static Color BrushColor(object? value, string message)
    {
        Check(value is ISolidColorBrush, message);
        return ((ISolidColorBrush)value!).Color;
    }

    private static bool AtLeastDesired(Control control) =>
        control.Bounds.Width + control.Margin.Left + control.Margin.Right >= control.DesiredSize.Width &&
        control.Bounds.Height + control.Margin.Top + control.Margin.Bottom >= control.DesiredSize.Height;

    private static bool Contains(Rect outer, Rect inner) =>
        inner.Left >= outer.Left && inner.Top >= outer.Top && inner.Right <= outer.Right && inner.Bottom <= outer.Bottom;

    private static double IntersectionArea(Rect left, Rect right)
    {
        var width = Math.Min(left.Right, right.Right) - Math.Max(left.Left, right.Left);
        var height = Math.Min(left.Bottom, right.Bottom) - Math.Max(left.Top, right.Top);
        return width > 0 && height > 0 ? width * height : 0;
    }

    private static bool IsTwo(Thickness thickness) =>
        thickness.Left == 2 && thickness.Top == 2 && thickness.Right == 2 && thickness.Bottom == 2;

    private static double Contrast(Color left, Color right)
    {
        var brighter = Math.Max(Luminance(left), Luminance(right));
        var darker = Math.Min(Luminance(left), Luminance(right));
        return (brighter + 0.05) / (darker + 0.05);
    }

    private static double Luminance(Color color) =>
        0.2126 * Linear(color.R) + 0.7152 * Linear(color.G) + 0.0722 * Linear(color.B);

    private static double Linear(byte channel)
    {
        var value = channel / 255d;
        return value <= 0.04045 ? value / 12.92 : Math.Pow((value + 0.055) / 1.055, 2.4);
    }

    private static Rect AbsoluteBounds(Grid parent, Control child) =>
        new(parent.Bounds.X + child.Bounds.X, parent.Bounds.Y + child.Bounds.Y, child.Bounds.Width, child.Bounds.Height);

    private static Point WorldPixel(ExistingUiTree tree, double x, double y)
    {
        var viewport = BoundsIn(tree.Viewport, tree.Window);
        return new Point(viewport.X + x, viewport.Y + viewport.Height - y);
    }

    private static PixelRect PixelBounds(Rect bounds)
    {
        var left = checked((int)Math.Floor(bounds.Left));
        var top = checked((int)Math.Floor(bounds.Top));
        var right = checked((int)Math.Ceiling(bounds.Right));
        var bottom = checked((int)Math.Ceiling(bounds.Bottom));
        return new PixelRect(left, top, right - left, bottom - top);
    }

    private static void CheckInsideFrame(PixelRect bounds, PixelSize size, string message) =>
        Check(bounds.X >= 0 && bounds.Y >= 0 && bounds.Right <= size.Width && bounds.Bottom <= size.Height, message);

    private static void CheckColorNear(Frame frame, Point point, Color color, int radius, string message)
    {
        for (var y = (int)Math.Round(point.Y) - radius; y <= (int)Math.Round(point.Y) + radius; y++)
        {
            for (var x = (int)Math.Round(point.X) - radius; x <= (int)Math.Round(point.X) + radius; x++)
            {
                if (IsColor(frame, x, y, color))
                {
                    return;
                }
            }
        }

        var matches = new List<Point>();
        for (var y = 0; y < frame.PixelSize.Height; y++)
        {
            for (var x = 0; x < frame.PixelSize.Width; x++)
            {
                if (IsColor(frame, x, y, color))
                {
                    matches.Add(new Point(x, y));
                }
            }
        }

        var bounds = matches.Count == 0
            ? "none"
            : $"{matches.Min(item => item.X)},{matches.Min(item => item.Y)}..{matches.Max(item => item.X)},{matches.Max(item => item.Y)} ({matches.Count})";
        var closest = matches.Count == 0
            ? "none"
            : matches.MinBy(item => Math.Pow(item.X - point.X, 2) + Math.Pow(item.Y - point.Y, 2)).ToString();
        var samples = new HashSet<string>();
        for (var y = (int)Math.Round(point.Y) - radius; y <= (int)Math.Round(point.Y) + radius; y++)
        {
            for (var x = (int)Math.Round(point.X) - radius; x <= (int)Math.Round(point.X) + radius; x++)
            {
                if (x >= 0 && y >= 0 && x < frame.PixelSize.Width && y < frame.PixelSize.Height)
                {
                    var offset = checked((y * frame.PixelSize.Width + x) * 4);
                    samples.Add(Convert.ToHexString(frame.Pixels.AsSpan(offset, 4)));
                }
            }
        }

        Check(false, $"{message}; expected {point}; closest {closest}; exact color bounds {bounds}; samples {string.Join(',', samples)}");
    }

    private static void CheckColorCount(Frame frame, Point point, Color color, int radius, int minimum, string message)
    {
        Check(ColorCount(frame, point, color, radius) >= minimum, message);
    }

    private static int ColorCount(Frame frame, Point point, Color color, int radius)
    {
        var count = 0;
        for (var y = (int)Math.Round(point.Y) - radius; y <= (int)Math.Round(point.Y) + radius; y++)
        {
            for (var x = (int)Math.Round(point.X) - radius; x <= (int)Math.Round(point.X) + radius; x++)
            {
                count += IsColor(frame, x, y, color) ? 1 : 0;
            }
        }

        return count;
    }

    private static bool IsColor(Frame frame, int x, int y, Color color)
    {
        if (x < 0 || y < 0 || x >= frame.PixelSize.Width || y >= frame.PixelSize.Height)
        {
            return false;
        }

        var offset = checked((y * frame.PixelSize.Width + x) * 4);
        return frame.Pixels[offset + 1] == color.G &&
            frame.Pixels[offset + 3] == color.A &&
            ((frame.Pixels[offset] == color.B && frame.Pixels[offset + 2] == color.R) ||
             (frame.Pixels[offset] == color.R && frame.Pixels[offset + 2] == color.B));
    }

    private static void CheckGridDiff(byte[] full, byte[] withoutGrid, PixelSize size, PixelRect viewport)
    {
        var diff = PixelDiff(full, withoutGrid, size, "grid diff");
        var columns = new int[size.Width];
        var rows = new int[size.Height];
        for (var index = 0; index < diff.Length; index++)
        {
            if (!diff[index])
            {
                continue;
            }

            var x = index % size.Width;
            var y = index / size.Width;
            Check(x >= viewport.X && x < viewport.Right && y >= viewport.Y && y < viewport.Bottom, "grid diff outside viewport");
            columns[x]++;
            rows[y]++;
        }

        var verticalBands = BandCenters(columns, Math.Max(1, viewport.Height / 2));
        var horizontalBands = BandCenters(rows, Math.Max(1, viewport.Width / 2));
        Check(verticalBands.Length >= 3 && horizontalBands.Length >= 3, "grid bands");
        CheckSpacing(verticalBands, 17.25, "grid vertical spacing");
        CheckSpacing(horizontalBands, 17.25, "grid horizontal spacing");
    }

    private static void CheckUcsDiff(byte[] full, byte[] withoutUcs, PixelSize size, PixelRect viewport)
    {
        var diff = PixelDiff(full, withoutUcs, size, "UCS diff");
        var minXByRow = Enumerable.Repeat(int.MaxValue, size.Height).ToArray();
        var maxXByRow = Enumerable.Repeat(int.MinValue, size.Height).ToArray();
        var minYByColumn = Enumerable.Repeat(int.MaxValue, size.Width).ToArray();
        var maxYByColumn = Enumerable.Repeat(int.MinValue, size.Width).ToArray();
        var minX = viewport.X + 20;
        var maxX = viewport.X + 105;
        var minY = viewport.Bottom - 150;
        var maxY = viewport.Bottom - 60;
        for (var index = 0; index < diff.Length; index++)
        {
            if (!diff[index])
            {
                continue;
            }

            var x = index % size.Width;
            var y = index / size.Width;
            Check(x >= minX && x <= maxX && y >= minY && y <= maxY, "UCS diff bounds");
            minXByRow[y] = Math.Min(minXByRow[y], x);
            maxXByRow[y] = Math.Max(maxXByRow[y], x);
            minYByColumn[x] = Math.Min(minYByColumn[x], y);
            maxYByColumn[x] = Math.Max(maxYByColumn[x], y);
        }

        var horizontalRows = new bool[size.Height];
        var verticalColumns = new bool[size.Width];
        for (var y = minY; y <= maxY; y++)
        {
            horizontalRows[y] = minXByRow[y] != int.MaxValue && maxXByRow[y] - minXByRow[y] + 1 >= 28;
        }

        for (var x = minX; x <= maxX; x++)
        {
            verticalColumns[x] = minYByColumn[x] != int.MaxValue && maxYByColumn[x] - minYByColumn[x] + 1 >= 28;
        }

        Check(horizontalRows.Any(value => value) && verticalColumns.Any(value => value), "UCS arms");
        for (var y = minY; y <= maxY; y++)
        {
            for (var x = minX; x <= maxX; x++)
            {
                if (horizontalRows[y] && verticalColumns[x] && diff[y * size.Width + x])
                {
                    return;
                }
            }
        }

        Check(false, "UCS arm intersection");
    }

    private static bool[] PixelDiff(byte[] left, byte[] right, PixelSize size, string message)
    {
        var expectedLength = checked(size.Width * size.Height * 4);
        Check(left.Length == expectedLength && right.Length == expectedLength, $"{message} pixel buffer");
        var diff = new bool[size.Width * size.Height];
        var found = false;
        for (var pixel = 0; pixel < diff.Length; pixel++)
        {
            var offset = pixel * 4;
            diff[pixel] = left[offset] != right[offset] || left[offset + 1] != right[offset + 1] ||
                left[offset + 2] != right[offset + 2] || left[offset + 3] != right[offset + 3];
            found |= diff[pixel];
        }

        Check(found, message);
        return diff;
    }

    private static void CheckDiffInside(byte[] before, byte[] after, PixelSize size, PixelRect bounds, string message)
    {
        var diff = PixelDiff(before, after, size, message);
        for (var index = 0; index < diff.Length; index++)
        {
            if (!diff[index])
            {
                continue;
            }

            var x = index % size.Width;
            var y = index / size.Width;
            Check(x >= bounds.X && x < bounds.Right && y >= bounds.Y && y < bounds.Bottom, $"{message} outside bounds");
        }
    }

    private static void CheckDiffExistsInside(
        byte[] before,
        byte[] after,
        PixelSize size,
        PixelRect bounds,
        string message)
    {
        CheckInsideFrame(bounds, size, $"{message} bounds");
        for (var y = bounds.Y; y < bounds.Bottom; y++)
        {
            for (var x = bounds.X; x < bounds.Right; x++)
            {
                var offset = checked((y * size.Width + x) * 4);
                if (!before.AsSpan(offset, 4).SequenceEqual(after.AsSpan(offset, 4)))
                {
                    return;
                }
            }
        }

        throw new InvalidOperationException(message);
    }

    private static void CheckPixelsEqualInside(Frame before, Frame after, PixelRect bounds, string message)
    {
        Check(before.PixelSize == after.PixelSize, $"{message} pixel size");
        CheckInsideFrame(bounds, before.PixelSize, $"{message} bounds");
        for (var y = bounds.Y; y < bounds.Bottom; y++)
        {
            var offset = checked((y * before.PixelSize.Width + bounds.X) * 4);
            var length = checked(bounds.Width * 4);
            Check(
                before.Pixels.AsSpan(offset, length).SequenceEqual(after.Pixels.AsSpan(offset, length)),
                message);
        }
    }

    private static void CheckNoTemporaryFiles(string directory, string message) =>
        Check(!Directory.EnumerateFiles(directory, "*.tmp", SearchOption.TopDirectoryOnly).Any(), message);

    private static double[] BandCenters(int[] counts, int minimumCount)
    {
        var centers = new List<double>();
        for (var index = 0; index < counts.Length; index++)
        {
            if (counts[index] < minimumCount)
            {
                continue;
            }

            var start = index;
            while (index + 1 < counts.Length && counts[index + 1] >= minimumCount)
            {
                index++;
            }

            centers.Add((start + index) / 2d);
        }

        return centers.ToArray();
    }

    private static void CheckSpacing(double[] centers, double unit, string message)
    {
        for (var index = 1; index < centers.Length; index++)
        {
            var spacing = centers[index] - centers[index - 1];
            var gridSteps = Math.Max(1, (int)Math.Round(spacing / unit));
            Check(
                Math.Abs(spacing - gridSteps * unit) <= 1,
                $"{message}: {string.Join(',', centers.Select(value => value.ToString("F1")))}");
        }
    }

    private static Frame CaptureFrame(Window window)
    {
        using var priorFrame = window.CaptureRenderedFrame();
        window.InvalidateVisual();
        Dispatcher.UIThread.RunJobs();
        using var bitmap = window.CaptureRenderedFrame();
        Check(bitmap is not null, "rendered frame");
        var pixelSize = bitmap!.PixelSize;
        Check(pixelSize.Width > 0 && pixelSize.Height > 0, "rendered frame dimensions");
        var stride = checked(pixelSize.Width * 4);
        var pixels = new byte[checked(stride * pixelSize.Height)];
        var pinned = GCHandle.Alloc(pixels, GCHandleType.Pinned);
        try
        {
            bitmap.CopyPixels(new PixelRect(0, 0, pixelSize.Width, pixelSize.Height), pinned.AddrOfPinnedObject(), pixels.Length, stride);
        }
        finally
        {
            pinned.Free();
        }

        using var png = new MemoryStream();
        bitmap.Save(png);
        var pngBytes = png.ToArray();
        return new Frame(Convert.ToHexString(SHA256.HashData(pngBytes)), pixelSize, pixels, pngBytes);
    }

    private static void SaveDemoFrame(string? capturePath, string clip, int index, Frame frame)
    {
        if (capturePath is null)
        {
            return;
        }

        var directory = Path.Combine(Path.GetDirectoryName(capturePath)!, "frames", clip);
        Directory.CreateDirectory(directory);
        File.WriteAllBytes(Path.Combine(directory, $"{index:000}.png"), frame.Png);
    }

    private static string FileHash(string path)
    {
        using var stream = File.OpenRead(path);
        return Convert.ToHexString(SHA256.HashData(stream)).ToLowerInvariant();
    }

    private static string FindModulePath(Process process, string moduleName)
    {
        foreach (ProcessModule module in process.Modules)
        {
            if (string.Equals(module.ModuleName, moduleName, StringComparison.OrdinalIgnoreCase))
            {
                return Path.GetFullPath(module.FileName);
            }
        }

        throw new InvalidOperationException($"loaded module not found: {moduleName}");
    }

    private static bool PathsEqual(string left, string right) =>
        string.Equals(Path.GetFullPath(left), Path.GetFullPath(right), StringComparison.OrdinalIgnoreCase);

    private static TException ExpectThrows<TException>(Action action, string message)
        where TException : Exception
    {
        try
        {
            action();
        }
        catch (TException error)
        {
            return error;
        }

        throw new InvalidOperationException(message);
    }

    private static long[] NormalizedShape(ReadOnlySpan<float> vertices)
    {
        if (vertices.Length < 4 || (vertices.Length & 1) != 0)
        {
            throw new InvalidOperationException("A rendered path needs finite XY pairs.");
        }

        var minX = double.PositiveInfinity;
        var minY = double.PositiveInfinity;
        var maxX = double.NegativeInfinity;
        var maxY = double.NegativeInfinity;
        for (var index = 0; index < vertices.Length; index += 2)
        {
            minX = Math.Min(minX, vertices[index]);
            maxX = Math.Max(maxX, vertices[index]);
            minY = Math.Min(minY, vertices[index + 1]);
            maxY = Math.Max(maxY, vertices[index + 1]);
        }

        var width = maxX - minX;
        var height = maxY - minY;
        if (!double.IsFinite(width) || !double.IsFinite(height) || width <= 0 || height <= 0)
        {
            throw new InvalidOperationException("A rendered path needs finite non-zero extents.");
        }

        var normalized = new long[vertices.Length];
        for (var index = 0; index < vertices.Length; index += 2)
        {
            normalized[index] = (long)Math.Round((vertices[index] - minX) / width * 1_000_000);
            normalized[index + 1] = (long)Math.Round((vertices[index + 1] - minY) / height * 1_000_000);
        }

        return normalized;
    }

    private static void Check(bool condition, string message)
    {
        _checkCount++;
        if (!condition)
        {
            throw new InvalidOperationException(message);
        }
    }

    private sealed record ExistingUiTree(
        MainWindow Window,
        Grid Root,
        Border ApplicationChrome,
        Border TitleBar,
        Grid TitleLayout,
        StackPanel QuickAccessToolbar,
        Grid Ribbon,
        Button HomeRibbonTab,
        Button ToolsRibbonTab,
        ItemsControl RibbonContext,
        Grid Workspace,
        CadViewport Viewport,
        Border PropertiesDock,
        StackPanel Properties,
        ComboBox PropertiesSelection,
        TextBlock PropertyType,
        TextBlock PropertyId,
        TextBlock PropertyLength,
        TextBlock UnsupportedProperty,
        TextBlock StatusText,
        TextBlock DocumentTitle,
        Grid CommandBar,
        TextBox CommandInput,
        TextBox GlobalSearch,
        StackPanel LayoutTabs,
        Button ModelLayout,
        Button GridStatus,
        Button UcsButton,
        Button OsnapStatus,
        Button OrthoStatus,
        Button ViewportMenu,
        Button PropertiesRibbon,
        Button CloseProperties,
        Button LayerManagerButton,
        Button CloseLayers,
        Border LayerManager,
        Button NewButton,
        Button OpenButton,
        Button SaveButton,
        Button LineButton,
        Button RailLineButton,
        Button UndoButton,
        Button RedoButton,
        Button NewDocumentButton,
        Button NewWorkspaceDocumentButton,
        Button SelectToolButton,
        Button PolylineButton,
        Button RectangleButton,
        Button CircleButton,
        Button ArcButton,
        Button MultilineButton,
        Button MoveButton,
        Button RotateButton,
        Button CopyButton,
        Button MirrorButton,
        Button ArrayButton,
        Button DimensionButton,
        Button LineweightStatus,
        Button AnnotationStatus,
        Button ZoomInButton,
        Button PanButton,
        Button FitViewButton,
        Button ResetViewButton,
        Button LayerRowsToggle,
        Grid LayerRows,
        Button GeneralSection,
        StackPanel GeneralRows,
        Button ViewSection,
        StackPanel ViewRows,
        Button MiscSection,
        StackPanel MiscRows,
        Button UnsupportedButton);

    private sealed record UiTree(
        Grid Shell,
        Border Product,
        Grid ProductContent,
        TextBlock Heading,
        TextBlock ProductDescriptor,
        TextBlock DocumentLabel,
        Border Ribbon,
        Grid Toolbar,
        Border FileGroup,
        Grid FileContent,
        TextBlock FileLabel,
        StackPanel FileButtons,
        Border DrawGroup,
        Grid DrawContent,
        TextBlock DrawLabel,
        Border HistoryGroup,
        Grid HistoryContent,
        TextBlock HistoryLabel,
        StackPanel HistoryButtons,
        Button NewButton,
        Button OpenButton,
        Button SaveButton,
        Button LineButton,
        Button UndoButton,
        Button RedoButton,
        Grid Workspace,
        CadViewport Viewport,
        Border Inspector,
        Grid InspectorContent,
        TextBlock PropertiesHeading,
        TextBlock SelectionContext,
        TextBlock SelectionHelp,
        Border Properties,
        StackPanel PropertiesContent,
        TextBlock PropertyType,
        TextBlock PropertyId,
        TextBlock PropertyLength,
        Border Status,
        Grid StatusContent,
        Grid CommandRow,
        TextBlock CommandLabel,
        TextBlock StatusText,
        Grid StatusLine,
        TextBlock ModelStatus,
        TextBlock GridStatus,
        TextBlock UcsStatus,
        TextBlock SnapStatus,
        TextBlock ShortcutStatus);

    private readonly record struct Frame(string PngHash, PixelSize PixelSize, byte[] Pixels, byte[] Png);
}
