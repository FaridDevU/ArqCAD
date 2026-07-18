using System.Text;
using ArcForge.Native;

namespace ArcForge.Desktop.Frontend;

public sealed class WorkspaceSession : IDisposable
{
    private static readonly UTF8Encoding StrictUtf8 = new(
        encoderShouldEmitUTF8Identifier: false,
        throwOnInvalidBytes: true);

    private ArcCadSession _session;
    private ArcCadRenderBatchKey? _lineBatchKey;
    private CadEntityPath[] _entities = [];
    private CadMarker[] _markers = [];
    private CadLine[] _lines = [];
    private ulong _nextTransactionSequence;
    private int _undoDepth;
    private int _redoDepth;

    public WorkspaceSession(string? aliasFilePath = null)
    {
        AliasFilePath = ResolveAliasFilePath(aliasFilePath);
        _session = CreateEmptySession();
        if (!File.Exists(AliasFilePath))
        {
            return;
        }

        try
        {
            ReloadAliases();
        }
        catch (Exception error)
        {
            LastAliasError = error.Message;
        }
    }

    public bool IsBackendConnected => !_session.IsDisposed;

    public ulong EntityId { get; private set; }

    public ulong? LastTransactionSequence { get; private set; }

    public ReadOnlyMemory<CadLine> Lines => _lines;

    public ReadOnlyMemory<CadEntityPath> Entities => _entities;

    public ReadOnlyMemory<CadMarker> Markers => _markers;

    public int EntityCount => _entities.Length + _markers.Length;

    public bool CanUndo => _undoDepth > 0;

    public bool CanRedo => _redoDepth > 0;

    public string? CurrentPath { get; private set; }

    public bool IsDirty { get; private set; }

    public string AliasFilePath { get; }

    public string AliasContent { get; private set; } = string.Empty;

    public string? LastAliasError { get; private set; }

    public string? LastAliasMessage { get; private set; }

    public string ResolveCommandAlias(string token)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(token);
        return ResolveKnownCommand(token) ?? token;
    }

    public string? ResolveKnownCommand(string token)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(token);
        return _session.ResolveAlias(token);
    }

    public string ReloadAliases()
    {
        try
        {
            var content = ReadAliasContent(AliasFilePath);
            var message = _session.ReinitializeAliases(content);
            AliasContent = content;
            LastAliasError = null;
            LastAliasMessage = message;
            return message;
        }
        catch (Exception error)
        {
            LastAliasError = error.Message;
            throw;
        }
    }

    public void EnsureAliasFile()
    {
        if (!File.Exists(AliasFilePath))
        {
            _ = SaveAliases(AliasContent);
        }
    }

    public string SaveAliases(string content)
    {
        ArgumentNullException.ThrowIfNull(content);
        var directory = Path.GetDirectoryName(AliasFilePath)!;
        Directory.CreateDirectory(directory);
        var temporaryPath = Path.Combine(
            directory,
            $".{Path.GetFileName(AliasFilePath)}.{Guid.NewGuid():N}.tmp");
        var previousContent = AliasContent;
        try
        {
            var bytes = StrictUtf8.GetBytes(content);
            using (var stream = new FileStream(
                temporaryPath,
                FileMode.CreateNew,
                FileAccess.Write,
                FileShare.None,
                4096,
                FileOptions.WriteThrough))
            {
                stream.Write(bytes);
                stream.Flush(flushToDisk: true);
            }

            var message = _session.ReinitializeAliases(content);
            try
            {
                if (File.Exists(AliasFilePath))
                {
                    File.Replace(temporaryPath, AliasFilePath, null);
                }
                else
                {
                    File.Move(temporaryPath, AliasFilePath);
                }
            }
            catch (Exception commitError)
            {
                try
                {
                    _ = _session.ReinitializeAliases(previousContent);
                }
                catch (Exception rollbackError)
                {
                    throw new AggregateException(
                        "No se pudo guardar el archivo PGP ni restaurar la tabla activa.",
                        commitError,
                        rollbackError);
                }

                throw;
            }

            AliasContent = content;
            LastAliasError = null;
            LastAliasMessage = message;
            return message;
        }
        catch (Exception error)
        {
            LastAliasError = error.Message;
            throw;
        }
        finally
        {
            if (File.Exists(temporaryPath))
            {
                File.Delete(temporaryPath);
            }
        }
    }

    public ulong? SelectedEntityId { get; private set; }

    public CadLine? SelectedLine
    {
        get
        {
            if (SelectedEntityId is not { } id)
            {
                return null;
            }

            var line = _lines.FirstOrDefault(candidate => candidate.EntityId == id);
            return line.EntityId == 0 ? null : line;
        }
    }

    public CadEntityPath? SelectedEntity
    {
        get
        {
            if (SelectedEntityId is not { } id)
            {
                return null;
            }

            var entity = _entities.FirstOrDefault(candidate => candidate.EntityId == id);
            return entity.EntityId == 0 ? null : entity;
        }
    }

    public CadMarker? SelectedMarker
    {
        get
        {
            if (SelectedEntityId is not { } id)
            {
                return null;
            }

            var marker = _markers.FirstOrDefault(candidate => candidate.EntityId == id);
            return marker.EntityId == 0 ? null : marker;
        }
    }

    public string? SelectedEntityType => SelectedEntityId is { } id
        ? ParseEntityType(_session.ListEntities([id]), id)
        : null;

    public IReadOnlyList<ArcCadLayerInfo> Layers => _session.Layers();

    public ArcCadLayerInfo ResolveLayer(string reference)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(reference);
        var layers = _session.Layers();
        ArcCadLayerInfo[] matches;
        if (ulong.TryParse(reference, out var id))
        {
            matches = layers.Where(layer => layer.Id == id).ToArray();
        }
        else
        {
            matches = layers
                .Where(layer => string.Equals(layer.Name, reference, StringComparison.OrdinalIgnoreCase))
                .ToArray();
        }

        return matches.Length == 1
            ? matches[0]
            : throw new ArgumentException($"Capa inexistente: {reference}", nameof(reference));
    }

    public void CreateLayer(string name)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(name);
        ApplyLayerMutation(
            () => _session.CreateLayer(name),
            (before, after) =>
                after.Length == before.Length + 1 &&
                before.All(after.Contains) &&
                after.Count(layer => !before.Contains(layer)) == 1 &&
                after.Single(layer => !before.Contains(layer)) is { } created &&
                string.Equals(created.Name, name, StringComparison.Ordinal) &&
                !created.Off && !created.Frozen && !created.Locked && created.Plot && !created.Current);
    }

    public void DeleteLayer(ulong layerId)
    {
        var before = RequireLayer(layerId);
        ApplyLayerMutation(
            () => _session.DeleteLayer(layerId),
            (layers, after) => after.SequenceEqual(layers.Where(layer => layer.Id != before.Id)));
    }

    public void RenameLayer(ulong layerId, string name)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(name);
        _ = RequireLayer(layerId);
        ApplyLayerMutation(
            () => _session.RenameLayer(layerId, name),
            (before, after) => after.SequenceEqual(
                before.Select(layer => layer.Id == layerId ? layer with { Name = name } : layer)));
    }

    public void SetCurrentLayer(ulong layerId)
    {
        var target = RequireLayer(layerId);
        if (target.Current)
        {
            throw new InvalidOperationException("La capa ya es la actual.");
        }

        ApplyLayerMutation(
            () => _session.SetCurrentLayer(layerId),
            (before, after) => after.SequenceEqual(
                before.Select(layer => layer with { Current = layer.Id == layerId })));
    }

    public void SetLayerOff(ulong layerId, bool value) =>
        SetLayerFlag(layerId, value, layer => layer.Off, _session.SetLayerOff,
            (layer, state) => layer with { Off = state });

    public void SetLayerFrozen(ulong layerId, bool value) =>
        SetLayerFlag(layerId, value, layer => layer.Frozen, _session.SetLayerFrozen,
            (layer, state) => layer with { Frozen = state });

    public void SetLayerLocked(ulong layerId, bool value) =>
        SetLayerFlag(layerId, value, layer => layer.Locked, _session.SetLayerLocked,
            (layer, state) => layer with { Locked = state });

    public void SetLayerPlot(ulong layerId, bool value) =>
        SetLayerFlag(layerId, value, layer => layer.Plot, _session.SetLayerPlot,
            (layer, state) => layer with { Plot = state });

    public ArcCadPoint ResolvePoint(ArcCadPoint raw, double radius, out ArcCadSnap? snap)
    {
        var candidates = _session.Snap(raw, radius);
        snap = candidates.Count == 0 ? null : candidates[0];
        return snap?.Point ?? raw;
    }

    public void CreateLine(ArcCadPoint start, ArcCadPoint end)
    {
        var previousEntities = _entities;
        var previousMarkers = _markers;
        var previousLayers = _session.Layers().ToArray();
        try
        {
            var line = _session.CreateLine(start, end);
            if (line.TransactionSequence != _nextTransactionSequence || line.EntityId == 0)
            {
                throw new InvalidOperationException("Native LINE returned invalid transaction metadata.");
            }

            var parsed = ParseFullScene(_session, _session.RenderFull());
            var nextLines = parsed.Lines;
            if (nextLines.Length != _lines.Length + 1 ||
                parsed.Entities.Length + parsed.Markers.Length !=
                    previousEntities.Length + previousMarkers.Length + 1 ||
                !nextLines.Any(item => item.EntityId == line.EntityId) ||
                !_lines.All(previous => nextLines.Contains(previous)) ||
                !ScenePreserved(
                    previousEntities,
                    previousMarkers,
                    parsed.Entities,
                    parsed.Markers))
            {
                throw new InvalidOperationException("Native LINE render delta did not preserve the scene plus its created entity.");
            }

            _entities = parsed.Entities;
            _markers = parsed.Markers;
            _lines = nextLines;
            _lineBatchKey = parsed.BatchKey;
            EntityId = line.EntityId;
            LastTransactionSequence = line.TransactionSequence;
            _nextTransactionSequence = checked(_nextTransactionSequence + 1);
            _undoDepth = checked(_undoDepth + 1);
            _redoDepth = 0;
            IsDirty = true;
        }
        catch (ArcCadCommandException)
        {
            VerifyRejectedMutation(previousEntities, previousMarkers, previousLayers);
            throw;
        }
        catch
        {
            _session.Dispose();
            throw;
        }
    }

    public void CreateCircle(ArcCadPoint center, ArcCadPoint radiusPoint)
    {
        var dx = radiusPoint.X - center.X;
        var dy = radiusPoint.Y - center.Y;
        var radius = Math.Sqrt(dx * dx + dy * dy);
        if (!double.IsFinite(radius) || radius <= 0.000001)
        {
            throw new ArgumentException("El círculo necesita un radio mayor que cero.", nameof(radiusPoint));
        }

        CreateNativeEntity(() => _session.CreateCircle(center, radius), "CIRCLE");
    }

    public void CreateCircleTwoPoints(ArcCadPoint first, ArcCadPoint second)
    {
        var result = _session.CreateCircleTwoPoints(first, second);
        CreateNativeEntity(() => result, "CIRCLE 2P");
    }

    public void CreateCircleThreePoints(ArcCadPoint first, ArcCadPoint second, ArcCadPoint third)
    {
        var result = _session.CreateCircleThreePoints(first, second, third);
        CreateNativeEntity(() => result, "CIRCLE 3P");
    }

    public void CreateTangentCircle(
        ulong firstEntityId,
        ArcCadPoint firstPick,
        ulong secondEntityId,
        ArcCadPoint secondPick,
        double radius)
    {
        RequireOtherVisibleEntity(firstEntityId, secondEntityId);
        if (SelectedEntityId != secondEntityId)
        {
            throw new InvalidOperationException("La segunda LINE debe permanecer seleccionada.");
        }

        var result = _session.CreateTangentCircle(
            firstEntityId,
            firstPick,
            secondEntityId,
            secondPick,
            radius);
        ApplyMutation(() => result, expectedCreatedCount: 1);
    }

    public void CreatePoint(ArcCadPoint position)
    {
        if (!double.IsFinite(position.X) || !double.IsFinite(position.Y))
        {
            throw new ArgumentOutOfRangeException(nameof(position), "POINT coordinates must be finite.");
        }

        CreateNativeEntity(() => _session.CreatePoint(position), "POINT");
    }

    public void CreateArc(ArcCadPoint start, ArcCadPoint middle, ArcCadPoint end) =>
        CreateNativeEntity(() => _session.CreateArc(start, middle, end), "ARC");

    public void CreateArcCenterStartEnd(ArcCadPoint center, ArcCadPoint start, ArcCadPoint end)
    {
        var result = _session.CreateArcCenterStartEnd(center, start, end);
        CreateNativeEntity(() => result, "ARC CSE");
    }

    public void CreateEllipse(ArcCadPoint center, ArcCadPoint axisEnd, double ratio)
    {
        var result = _session.CreateEllipse(center, axisEnd, ratio);
        CreateNativeEntity(() => result, "ELLIPSE");
    }

    public void CreateEllipticalArc(
        ArcCadPoint center,
        ArcCadPoint axisEnd,
        double ratio,
        double startParameterRadians,
        double endParameterRadians)
    {
        var result = _session.CreateEllipticalArc(
            center,
            axisEnd,
            ratio,
            startParameterRadians,
            endParameterRadians);
        CreateNativeEntity(() => result, "ELLIPSE ARC");
    }

    public void CreateRectangle(
        ArcCadPoint first,
        ArcCadPoint opposite,
        double? chamfer1 = null,
        double? chamfer2 = null,
        double? fillet = null,
        double? width = null) =>
        CreateNativeEntity(
            () => _session.CreateRectangle(first, opposite, chamfer1, chamfer2, fillet, width),
            "RECTANG");

    public void CreatePolyline(IReadOnlyList<ArcCadPoint> points, bool closed = false) =>
        CreateNativeEntity(() => _session.CreatePolyline(points, closed), "PLINE");

    public void CreateSpline(IReadOnlyList<ArcCadPoint> points) =>
        CreateNativeEntity(() => _session.CreateSpline(points), "SPLINE");

    public void CreateRevisionCloud(
        IReadOnlyList<ArcCadPoint> contour,
        double arcLength,
        string style = "NORMAL")
    {
        ArgumentNullException.ThrowIfNull(contour);
        if (contour.Count < 3 ||
            contour.Any(point => !double.IsFinite(point.X) || !double.IsFinite(point.Y)))
        {
            throw new ArgumentException("REVCLOUD requires at least three finite contour points.", nameof(contour));
        }

        if (!double.IsFinite(arcLength) || arcLength <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(arcLength));
        }

        CreateNativeEntity(() => _session.CreateRevisionCloud(contour, arcLength, style), "REVCLOUD");
    }

    public void CreateWipeout(IReadOnlyList<ArcCadPoint> points)
    {
        ArgumentNullException.ThrowIfNull(points);
        if (points.Count < 3 ||
            points.Any(point => !double.IsFinite(point.X) || !double.IsFinite(point.Y)))
        {
            throw new ArgumentException("WIPEOUT requires at least three finite points.", nameof(points));
        }

        CreateNativeEntity(() => _session.CreateWipeout(points), "WIPEOUT");
    }

    public void CreateXline(ArcCadPoint start, ArcCadPoint through) =>
        CreateNativeEntity(() => _session.CreateXline(start, through), "XLINE");

    public void CreateHorizontalXline(ArcCadPoint point) =>
        CreateNativeEntity(() => _session.CreateHorizontalXline(point), "XLINE");

    public void CreateVerticalXline(ArcCadPoint point) =>
        CreateNativeEntity(() => _session.CreateVerticalXline(point), "XLINE");

    public void CreateAngledXline(ArcCadPoint point, double angleRadians) =>
        CreateNativeEntity(() => _session.CreateAngledXline(point, angleRadians), "XLINE");

    public void CreateRay(ArcCadPoint origin, ArcCadPoint through) =>
        CreateNativeEntity(() => _session.CreateRay(origin, through), "RAY");

    public void CreateDonut(ArcCadPoint center, double exteriorDiameter, double interiorDiameter) =>
        CreateNativeEntity(
            () => _session.CreateDonut(center, exteriorDiameter, interiorDiameter),
            "DONUT");

    public void CreatePolygon(
        int sides,
        ArcCadPoint center,
        ArcCadPoint radiusPoint,
        bool circumscribed)
    {
        if (sides is < 3 or > 1024)
        {
            throw new ArgumentOutOfRangeException(nameof(sides), "El polígono requiere entre 3 y 1024 lados.");
        }

        var dx = radiusPoint.X - center.X;
        var dy = radiusPoint.Y - center.Y;
        var radius = Math.Sqrt(dx * dx + dy * dy);
        if (!double.IsFinite(radius) || radius <= 0.000001)
        {
            throw new ArgumentException("El polígono necesita un radio mayor que cero.", nameof(radiusPoint));
        }

        CreateNativeEntity(
            () => _session.CreatePolygon((ulong)sides, center, radius, circumscribed),
            "POLYGON");
    }

    public string IdentifyPoint(ArcCadPoint point) => _session.IdentifyPoint(point);

    public string MeasureDistance(ArcCadPoint first, ArcCadPoint second) =>
        _session.MeasureDistance(first, second);

    public string MeasureAngle(ArcCadPoint vertex, ArcCadPoint firstRay, ArcCadPoint secondRay) =>
        _session.MeasureAngle(vertex, firstRay, secondRay);

    public string MeasureSelectedRadius() => SelectedEntityId is { } entityId
        ? _session.MeasureRadius([entityId])
        : throw new InvalidOperationException("Seleccione un CIRCLE o ARC antes de medir el radio.");

    public string MeasureSelectedLength() => SelectedEntityId is { } entityId
        ? _session.MeasureLength([entityId])
        : throw new InvalidOperationException("Seleccione una entidad antes de medir la longitud.");

    public string MeasureSelectedBounds() => SelectedEntityId is { } entityId
        ? _session.MeasureBounds([entityId])
        : throw new InvalidOperationException("Seleccione una entidad antes de medir sus limites.");

    public string ListSelectedEntity() => SelectedEntityId is { } entityId
        ? _session.ListEntities([entityId])
        : throw new InvalidOperationException("Seleccione una entidad antes de usar LIST.");

    public string MeasureSelectedArea() => SelectedEntityId is { } entityId
        ? _session.MeasureArea([entityId])
        : throw new InvalidOperationException("Seleccione una entidad antes de usar AREA.");

    public void MoveSelected(ArcCadPoint from, ArcCadPoint to)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(() => _session.MoveEntities([entityId], from, to), expectedCreatedCount: 0);
    }

    public void CopySelected(ArcCadPoint from, ArcCadPoint to)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(() => _session.CopyEntities([entityId], from, to), expectedCreatedCount: 1);
    }

    public void MirrorSelected(ArcCadPoint firstAxisPoint, ArcCadPoint secondAxisPoint)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.MirrorEntities([entityId], firstAxisPoint, secondAxisPoint),
            expectedCreatedCount: 1);
    }

    public void RotateSelected(ArcCadPoint basePoint, double angleRadians)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.RotateEntities([entityId], basePoint, angleRadians),
            expectedCreatedCount: 0);
    }

    public void EraseSelected()
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.EraseEntities([entityId]),
            expectedCreatedCount: 0,
            expectedRemovedEntityIds: [entityId]);
    }

    public int Oops()
    {
        var result = _session.Oops();
        ApplyMutation(
            () => result,
            expectedCreatedCount: null,
            requireSelection: false);
        return result.CreatedEntityIds.Length;
    }

    public void ExplodeSelected()
    {
        var entityId = RequireSelectedEntity();
        if (!ListSelectedEntity().StartsWith("LWPOLYLINE #", StringComparison.Ordinal))
        {
            throw new InvalidOperationException("EXPLODE requiere una LWPOLYLINE seleccionada.");
        }

        ApplyMutation(
            () => _session.ExplodeEntities([entityId]),
            expectedCreatedCount: null,
            expectedRemovedEntityIds: [entityId]);
    }

    public void ConvertRevisionCloudSelected(double arcLength, string style = "NORMAL")
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.ConvertRevisionCloud(entityId, arcLength, style),
            expectedCreatedCount: 1,
            expectedRemovedEntityIds: [entityId]);
    }

    public void ScaleSelected(ArcCadPoint basePoint, double factor)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.ScaleEntities([entityId], basePoint, factor),
            expectedCreatedCount: 0);
    }

    public void OffsetSelected(double distance, ArcCadPoint side)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.OffsetEntities([entityId], distance, side),
            expectedCreatedCount: 1);
    }

    public void TrimSelected(ArcCadPoint pick)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.TrimEntities([entityId], [], pick),
            expectedCreatedCount: null);
    }

    public void ExtendSelected(ArcCadPoint pick)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.ExtendEntities([entityId], [], pick),
            expectedCreatedCount: 0);
    }

    public void ChamferSelectedWith(ulong firstEntityId, double distance)
    {
        var secondEntityId = RequireSelectedEntity();
        RequireOtherVisibleEntity(firstEntityId, secondEntityId);
        ApplyMutation(
            () => _session.ChamferEntities([firstEntityId, secondEntityId], distance, distance),
            expectedCreatedCount: 1);
    }

    public void FilletSelectedWith(ulong firstEntityId, double radius)
    {
        var secondEntityId = RequireSelectedEntity();
        RequireOtherVisibleEntity(firstEntityId, secondEntityId);
        var result = _session.FilletEntities([firstEntityId, secondEntityId], radius);
        ApplyMutation(
            () => result,
            expectedCreatedCount: 1);
    }

    public void BreakSelected(ArcCadPoint first, ArcCadPoint second)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.BreakEntity(entityId, first, second),
            expectedCreatedCount: null);
    }

    public void BreakSelectedAtPoint(ArcCadPoint point)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.BreakEntityAtPoint(entityId, point),
            expectedCreatedCount: 1);
    }

    public void LengthenSelected(ArcCadPoint pick, double total)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.LengthenEntity(entityId, pick, total),
            expectedCreatedCount: 0);
    }

    public void StretchSelected(
        ArcCadPoint firstCorner,
        ArcCadPoint secondCorner,
        ArcCadPoint basePoint,
        ArcCadPoint destination)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.StretchEntities(
                [entityId],
                firstCorner,
                secondCorner,
                basePoint,
                destination),
            expectedCreatedCount: 0);
    }

    public void JoinSelectedWith(ulong keepEntityId)
    {
        var removeEntityId = RequireSelectedEntity();
        RequireOtherVisibleEntity(keepEntityId, removeEntityId);
        ApplyMutation(
            () => _session.JoinEntities([keepEntityId, removeEntityId]),
            expectedCreatedCount: 0,
            expectedRemovedEntityIds: [removeEntityId]);
    }

    public void AlignSelected(
        ArcCadPoint firstSource,
        ArcCadPoint firstDestination,
        ArcCadPoint secondSource,
        ArcCadPoint secondDestination)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.AlignEntities(
                [entityId],
                firstSource,
                firstDestination,
                secondSource,
                secondDestination),
            expectedCreatedCount: 0);
    }

    public void NudgeSelected(ArcCadPoint delta)
    {
        var entityId = RequireSelectedEntity();
        ApplyMutation(
            () => _session.NudgeEntities([entityId], delta),
            expectedCreatedCount: 0);
    }

    public bool OverkillVisible()
    {
        _ = RequireSelectedEntity();
        var entityIds = _lines.Select(line => line.EntityId).ToArray();
        if (entityIds.Length < 2)
        {
            throw new InvalidOperationException("OVERKILL necesita al menos dos LINE visibles.");
        }

        var result = _session.OverkillEntities(entityIds);
        if (result is null)
        {
            return false;
        }

        ApplyMutation(
            () => result.Value,
            expectedCreatedCount: 0,
            inferRemovedEntityIds: true);
        return true;
    }

    public void CreateRectangularArraySelected(ulong rows, ulong columns, ArcCadPoint spacing)
    {
        var entityId = RequireSelectedEntity();
        if (rows == 0 || columns == 0 || rows > ulong.MaxValue / columns)
        {
            throw new ArgumentOutOfRangeException(nameof(rows));
        }

        var cells = rows * columns;
        if (cells < 2 || cells - 1 > int.MaxValue)
        {
            throw new ArgumentOutOfRangeException(nameof(rows));
        }

        var expectedCreatedCount = (int)(cells - 1);
        ApplyMutation(
            () => _session.CreateRectangularArray([entityId], rows, columns, spacing),
            expectedCreatedCount);
    }

    public void Undo()
    {
        if (!CanUndo)
        {
            throw new InvalidOperationException("Nothing to undo.");
        }

        ApplyHistory(() => _session.Undo(), undo: true);
    }

    public void Redo()
    {
        if (!CanRedo)
        {
            throw new InvalidOperationException("Nothing to redo.");
        }

        ApplyHistory(() => _session.Redo(), undo: false);
    }

    private void SetLayerFlag(
        ulong layerId,
        bool value,
        Func<ArcCadLayerInfo, bool> read,
        Func<ulong, bool, ArcCadMutationResult> execute,
        Func<ArcCadLayerInfo, bool, ArcCadLayerInfo> update)
    {
        var target = RequireLayer(layerId);
        if (read(target) == value)
        {
            throw new InvalidOperationException("La capa ya tiene el estado solicitado.");
        }

        ApplyLayerMutation(
            () => execute(layerId, value),
            (before, after) => after.SequenceEqual(
                before.Select(layer => layer.Id == layerId ? update(layer, value) : layer)));
    }

    private ArcCadLayerInfo RequireLayer(ulong layerId)
    {
        if (layerId == 0)
        {
            throw new ArgumentOutOfRangeException(nameof(layerId));
        }

        return _session.Layers().SingleOrDefault(layer => layer.Id == layerId) is { Id: not 0 } layer
            ? layer
            : throw new ArgumentException($"Capa inexistente: {layerId}", nameof(layerId));
    }

    private void ApplyLayerMutation(
        Func<ArcCadMutationResult> execute,
        Func<ArcCadLayerInfo[], ArcCadLayerInfo[], bool> validate)
    {
        var beforeLayers = _session.Layers().ToArray();
        var previousEntities = _entities;
        var previousMarkers = _markers;
        try
        {
            var result = execute();
            if (result.TransactionSequence != _nextTransactionSequence ||
                result.CreatedEntityIds.Length != 0)
            {
                throw new InvalidOperationException("Native LAYER returned invalid transaction metadata.");
            }

            var afterLayers = _session.Layers().ToArray();
            if (!validate(beforeLayers, afterLayers))
            {
                throw new InvalidOperationException("Native LAYER result did not match the requested operation.");
            }

            var parsed = ParseFullScene(_session, _session.RenderFull());
            var nextIds = SceneIds(parsed.Entities, parsed.Markers);
            _entities = parsed.Entities;
            _markers = parsed.Markers;
            _lines = parsed.Lines;
            _lineBatchKey = parsed.BatchKey;
            LastTransactionSequence = result.TransactionSequence;
            _nextTransactionSequence = checked(_nextTransactionSequence + 1);
            _undoDepth = checked(_undoDepth + 1);
            _redoDepth = 0;
            SelectedEntityId = SelectedEntityId is { } selected && nextIds.Contains(selected)
                ? selected
                : null;
            IsDirty = true;
        }
        catch (ArcCadCommandException)
        {
            VerifyRejectedMutation(previousEntities, previousMarkers, beforeLayers);
            throw;
        }
        catch
        {
            _session.Dispose();
            throw;
        }
    }

    private void CreateNativeEntity(Func<ArcCadMutationResult> execute, string command)
    {
        var previousEntities = _entities;
        var previousMarkers = _markers;
        var previousLayers = _session.Layers().ToArray();
        try
        {
            var result = execute();
            var createdIds = result.CreatedEntityIds.Span;
            if (result.TransactionSequence != _nextTransactionSequence ||
                createdIds.Length != 1 ||
                createdIds[0] == 0)
            {
                throw new InvalidOperationException($"Native {command} returned invalid transaction metadata.");
            }

            var entityId = createdIds[0];
            var parsed = ParseFullScene(_session, _session.RenderFull());
            if (parsed.Entities.Length + parsed.Markers.Length !=
                    previousEntities.Length + previousMarkers.Length + 1 ||
                !SceneContains(parsed.Entities, parsed.Markers, entityId) ||
                !ScenePreserved(
                    previousEntities,
                    previousMarkers,
                    parsed.Entities,
                    parsed.Markers))
            {
                throw new InvalidOperationException(
                    $"Native {command} render delta did not preserve the scene plus one created entity.");
            }

            _entities = parsed.Entities;
            _markers = parsed.Markers;
            _lines = parsed.Lines;
            _lineBatchKey = parsed.BatchKey;
            EntityId = entityId;
            LastTransactionSequence = result.TransactionSequence;
            _nextTransactionSequence = checked(_nextTransactionSequence + 1);
            _undoDepth = checked(_undoDepth + 1);
            _redoDepth = 0;
            IsDirty = true;
        }
        catch (ArcCadCommandException)
        {
            VerifyRejectedMutation(previousEntities, previousMarkers, previousLayers);
            throw;
        }
        catch
        {
            _session.Dispose();
            throw;
        }
    }

    private ulong RequireSelectedEntity() => SelectedEntityId is { } entityId
        ? entityId
        : throw new InvalidOperationException("Seleccione una entidad antes de modificarla.");

    private void RequireOtherVisibleEntity(ulong firstEntityId, ulong secondEntityId)
    {
        if (firstEntityId == secondEntityId || !_lines.Any(line => line.EntityId == firstEntityId))
        {
            throw new InvalidOperationException("Seleccione una segunda LINE distinta.");
        }
    }

    private void ApplyMutation(
        Func<ArcCadMutationResult> execute,
        int? expectedCreatedCount,
        ulong[]? expectedRemovedEntityIds = null,
        bool inferRemovedEntityIds = false,
        bool requireSelection = true)
    {
        var previousEntities = _entities;
        var previousMarkers = _markers;
        var previousLayers = _session.Layers().ToArray();
        try
        {
            var previousIds = SceneIds(previousEntities, previousMarkers);
            var explicitRemovedIds = expectedRemovedEntityIds?.ToHashSet() ?? [];
            if (inferRemovedEntityIds && explicitRemovedIds.Count > 0 ||
                explicitRemovedIds.Count != (expectedRemovedEntityIds?.Length ?? 0) ||
                explicitRemovedIds.Any(entityId => entityId == 0 || !previousIds.Contains(entityId)))
            {
                throw new InvalidOperationException("Native mutation expected invalid removed IDs.");
            }

            ulong? selectedEntityId = requireSelection ? RequireSelectedEntity() : SelectedEntityId;
            var result = execute();
            var createdCount = result.CreatedEntityIds.Length;
            if (result.TransactionSequence != _nextTransactionSequence ||
                expectedCreatedCount is { } expected && createdCount != expected)
            {
                throw new InvalidOperationException("Native mutation returned invalid transaction metadata.");
            }

            var parsed = ParseFullScene(_session, _session.RenderFull());
            var nextEntities = parsed.Entities;
            var nextMarkers = parsed.Markers;
            var nextLines = parsed.Lines;
            var nextIds = SceneIds(nextEntities, nextMarkers);
            var removedIds = inferRemovedEntityIds
                ? previousIds.Where(entityId => !nextIds.Contains(entityId)).ToHashSet()
                : explicitRemovedIds;
            if (inferRemovedEntityIds && removedIds.Count == 0)
            {
                throw new InvalidOperationException("Native mutation reported a transaction without removals.");
            }

            var createdIds = result.CreatedEntityIds.ToArray();
            var discoveredIds = nextIds
                .Where(id => !previousIds.Contains(id))
                .ToHashSet();
            var expectedRemovedCount = removedIds.Count;
            var countMatches = nextIds.Count ==
                previousIds.Count + createdCount - expectedRemovedCount;
            var previousIdsPreserved = previousIds
                .Where(entityId => !removedIds.Contains(entityId))
                .All(nextIds.Contains);
            var createdIdsPresent = createdIds.All(nextIds.Contains);
            var createdIdsMatch = discoveredIds.SetEquals(createdIds);
            var removedIdsAbsent = removedIds.All(entityId => !nextIds.Contains(entityId));
            var mutationChangedScene = createdCount > 0 || expectedRemovedCount > 0 ||
                !SceneEquals(nextEntities, nextMarkers, previousEntities, previousMarkers);
            if (!countMatches || !previousIdsPreserved || !createdIdsPresent ||
                !createdIdsMatch || !removedIdsAbsent || !mutationChangedScene)
            {
                throw new InvalidOperationException(
                    "Native mutation render delta did not match its transaction result " +
                    $"(previous={previousIds.Count}, next={nextIds.Count}, " +
                    $"expectedCreated={expectedCreatedCount?.ToString() ?? "any"}, created=[{string.Join(',', createdIds)}], " +
                    $"expectedRemoved=[{string.Join(',', removedIds)}], " +
                    $"discovered=[{string.Join(',', discoveredIds)}], checks=" +
                    $"{countMatches}/{previousIdsPreserved}/{createdIdsPresent}/{createdIdsMatch}/{removedIdsAbsent}/{mutationChangedScene}, " +
                    $"nextIds=[{string.Join(',', nextIds)}], " +
                    $"selectedBefore={selectedEntityId}).");
            }

            _entities = nextEntities;
            _markers = nextMarkers;
            _lines = nextLines;
            _lineBatchKey = parsed.BatchKey;
            if (createdIds.Length > 0)
            {
                EntityId = createdIds[^1];
            }
            LastTransactionSequence = result.TransactionSequence;
            _nextTransactionSequence = checked(_nextTransactionSequence + 1);
            _undoDepth = checked(_undoDepth + 1);
            _redoDepth = 0;
            SelectedEntityId = selectedEntityId is { } preservedSelection &&
                nextIds.Contains(preservedSelection)
                    ? preservedSelection
                    : null;
            IsDirty = true;
        }
        catch (ArcCadCommandException)
        {
            VerifyRejectedMutation(previousEntities, previousMarkers, previousLayers);
            throw;
        }
        catch
        {
            _session.Dispose();
            throw;
        }
    }

    private void VerifyRejectedMutation(
        CadEntityPath[] previousEntities,
        CadMarker[] previousMarkers,
        ArcCadLayerInfo[] previousLayers)
    {
        try
        {
            var parsed = ParseFullScene(_session, _session.RenderFull());
            if (!SceneEquals(parsed.Entities, parsed.Markers, previousEntities, previousMarkers) ||
                !_session.Layers().SequenceEqual(previousLayers))
            {
                throw new InvalidOperationException("Native command error changed document state.");
            }
        }
        catch
        {
            _session.Dispose();
            throw;
        }
    }

    public void SelectAt(ArcCadPoint point, double tolerance)
    {
        var selection = _session.SelectAt(point, tolerance);
        if (selection.Any(id => !SceneContains(_entities, _markers, id)))
        {
            throw new InvalidOperationException("Native selection referenced an entity outside the visible scene.");
        }

        SelectedEntityId = selection.Count == 0 ? null : selection[0];
    }

    public void NewDocument()
    {
        ThrowIfDirty();
        var candidate = CreateEmptySession();
        var swapped = false;
        try
        {
            _ = candidate.ReinitializeAliases(AliasContent);
            var previous = SwapDocument(candidate, [], [], [], null, null);
            swapped = true;
            previous.Dispose();
        }
        finally
        {
            if (!swapped)
            {
                candidate.Dispose();
            }
        }
    }

    public void SaveToPath(string path)
    {
        var fullPath = NormalizeSavePath(path);
        var bytes = _session.SaveArcf();
        if (bytes.Length == 0)
        {
            throw new InvalidOperationException("Native session returned an empty .arcf document.");
        }

        var directory = Path.GetDirectoryName(fullPath)!;
        var temporaryPath = Path.Combine(
            directory,
            $".{Path.GetFileName(fullPath)}.{Guid.NewGuid():N}.tmp");
        try
        {
            using (var stream = new FileStream(
                temporaryPath,
                FileMode.CreateNew,
                FileAccess.Write,
                FileShare.None,
                4096,
                FileOptions.WriteThrough))
            {
                stream.Write(bytes);
                stream.Flush(flushToDisk: true);
            }

            if (File.Exists(fullPath))
            {
                File.Replace(temporaryPath, fullPath, null);
            }
            else
            {
                File.Move(temporaryPath, fullPath);
            }

            CurrentPath = fullPath;
            IsDirty = false;
        }
        finally
        {
            if (File.Exists(temporaryPath))
            {
                File.Delete(temporaryPath);
            }
        }
    }

    public IReadOnlyList<string> OpenFromPath(string path)
    {
        ThrowIfDirty();
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        var fullPath = Path.GetFullPath(path);
        var bytes = File.ReadAllBytes(fullPath);
        var candidate = CreateEmptySession();
        var swapped = false;
        try
        {
            _ = candidate.ReinitializeAliases(AliasContent);
            var warnings = candidate.OpenArcf(bytes).ToArray();
            var parsed = ParseFullScene(candidate, candidate.RenderFull());
            var previous = SwapDocument(
                candidate,
                parsed.Entities,
                parsed.Markers,
                parsed.Lines,
                parsed.BatchKey,
                fullPath);
            swapped = true;
            previous.Dispose();
            return warnings;
        }
        finally
        {
            if (!swapped)
            {
                candidate.Dispose();
            }
        }
    }

    public void Dispose() => _session.Dispose();

    private void ApplyHistory(Action command, bool undo)
    {
        try
        {
            command();
            var parsed = ParseFullScene(_session, _session.RenderFull());
            ulong? selectedEntityId = SelectedEntityId is { } id &&
                SceneContains(parsed.Entities, parsed.Markers, id)
                ? id
                : null;

            if (undo)
            {
                _undoDepth--;
                _redoDepth++;
            }
            else
            {
                _redoDepth--;
                _undoDepth++;
            }

            _entities = parsed.Entities;
            _markers = parsed.Markers;
            _lines = parsed.Lines;
            _lineBatchKey = parsed.BatchKey;
            SelectedEntityId = selectedEntityId;
            IsDirty = true;
        }
        catch
        {
            _session.Dispose();
            throw;
        }
    }

    private ArcCadSession SwapDocument(
        ArcCadSession candidate,
        CadEntityPath[] candidateEntities,
        CadMarker[] candidateMarkers,
        CadLine[] candidateLines,
        ArcCadRenderBatchKey? candidateBatchKey,
        string? path)
    {
        var previous = _session;
        _session = candidate;
        _lineBatchKey = candidateBatchKey;
        _entities = candidateEntities;
        _markers = candidateMarkers;
        _lines = candidateLines;
        _nextTransactionSequence = 0;
        _undoDepth = 0;
        _redoDepth = 0;
        EntityId = 0;
        LastTransactionSequence = null;
        SelectedEntityId = null;
        CurrentPath = path;
        IsDirty = false;
        return previous;
    }

    private static ArcCadSession CreateEmptySession()
    {
        var session = ArcCadSession.Create();
        try
        {
            var delta = session.RenderDelta();
            if (delta.Upserts.Length != 0 ||
                delta.Removes.Length != 0 ||
                delta.Vertices.Length != 0 ||
                !double.IsFinite(delta.LinetypeScale) ||
                delta.LinetypeScale <= 0)
            {
                throw new InvalidOperationException("Native session did not start with an empty render delta.");
            }

            return session;
        }
        catch
        {
            session.Dispose();
            throw;
        }
    }

    private static string ResolveAliasFilePath(string? explicitPath)
    {
        if (explicitPath is not null)
        {
            ArgumentException.ThrowIfNullOrWhiteSpace(explicitPath);
            return Path.GetFullPath(explicitPath);
        }

        var configuredPath = Environment.GetEnvironmentVariable("ARCFORGE_PGP_PATH");
        if (!string.IsNullOrWhiteSpace(configuredPath))
        {
            return Path.GetFullPath(configuredPath);
        }

        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        if (string.IsNullOrWhiteSpace(localAppData))
        {
            throw new DirectoryNotFoundException("No se pudo resolver LOCALAPPDATA para Aliases.pgp.");
        }

        return Path.Combine(localAppData, "ArcForge", "Aliases.pgp");
    }

    private static string ReadAliasContent(string path)
    {
        var bytes = File.ReadAllBytes(path);
        var offset = bytes.Length >= 3 &&
            bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF
                ? 3
                : 0;
        if (offset == 0 && bytes.Length >= 2 &&
            (bytes[0] == 0xFF && bytes[1] == 0xFE ||
             bytes[0] == 0xFE && bytes[1] == 0xFF))
        {
            throw new InvalidDataException("Aliases.pgp debe estar codificado como UTF-8.");
        }

        return StrictUtf8.GetString(bytes, offset, bytes.Length - offset);
    }

    private static string NormalizeSavePath(string path)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        var fullPath = Path.GetFullPath(path);
        var directory = Path.GetDirectoryName(fullPath);
        if (string.IsNullOrEmpty(directory) || !Directory.Exists(directory))
        {
            throw new DirectoryNotFoundException($"Save directory does not exist: {directory}");
        }

        if (Directory.Exists(fullPath))
        {
            throw new IOException($"Save destination is a directory: {fullPath}");
        }

        return fullPath;
    }

    private void ThrowIfDirty()
    {
        if (IsDirty)
        {
            throw new InvalidOperationException("Guarde los cambios antes de continuar.");
        }
    }

    private static (
        CadEntityPath[] Entities,
        CadMarker[] Markers,
        CadLine[] Lines,
        ArcCadRenderBatchKey? BatchKey) ParseFullScene(
        ArcCadSession session,
        ArcCadRenderDelta full)
    {
        if (full.Removes.Length != 0 ||
            !double.IsFinite(full.LinetypeScale) ||
            full.LinetypeScale <= 0)
        {
            throw InvalidDelta();
        }

        var batches = full.Upserts.ToArray();
        if (batches.Length == 0)
        {
            if (full.Vertices.Length != 0)
            {
                throw InvalidDelta();
            }

            return ([], [], [], null);
        }

        var keys = batches
            .Select(batch => new ArcCadRenderBatchKey(batch.LayerId, batch.Color, batch.LinetypeId))
            .ToArray();
        if (keys.Distinct().Count() != keys.Length)
        {
            throw InvalidDelta();
        }

        var first = batches[0];
        var combined = new ArcCadRenderBatch(
            first.LayerId,
            first.Color,
            first.LinetypeId,
            batches.SelectMany(batch => batch.Strips.ToArray()).ToArray().AsMemory(),
            batches.SelectMany(batch => batch.Markers.ToArray()).ToArray().AsMemory());
        var synthetic = new ArcCadRenderDelta(
            new[] { combined }.AsMemory(),
            ReadOnlyMemory<ArcCadRenderBatchKey>.Empty,
            full.Vertices,
            full.LinetypeScale);
        var parsed = ParseDelta(session, synthetic, null);
        return (parsed.Entities, parsed.Markers, parsed.Lines, null);
    }

    private static (
        CadEntityPath[] Entities,
        CadMarker[] Markers,
        CadLine[] Lines,
        ArcCadRenderBatchKey? BatchKey) ParseDocumentDelta(
        ArcCadSession session,
        ArcCadRenderDelta delta)
    {
        if (delta.Upserts.Length == 0 &&
            delta.Removes.Length == 0 &&
            delta.Vertices.Length == 0)
        {
            if (!double.IsFinite(delta.LinetypeScale) || delta.LinetypeScale <= 0)
            {
                throw InvalidDelta();
            }

            return ([], [], [], null);
        }

        return ParseDelta(session, delta, null);
    }

    private static (
        CadEntityPath[] Entities,
        CadMarker[] Markers,
        CadLine[] Lines,
        ArcCadRenderBatchKey? BatchKey) ParseDelta(
        ArcCadSession session,
        ArcCadRenderDelta delta,
        ArcCadRenderBatchKey? lineBatchKey)
    {
        if (!double.IsFinite(delta.LinetypeScale) || delta.LinetypeScale <= 0)
        {
            throw InvalidDelta();
        }

        if (delta.Upserts.Length == 0 &&
            delta.Removes.Length == 1 &&
            delta.Vertices.Length == 0)
        {
            if (lineBatchKey is null || delta.Removes.Span[0] != lineBatchKey.Value)
            {
                throw InvalidDelta();
            }

            return ([], [], [], lineBatchKey);
        }

        if (delta.Upserts.Length != 1 || delta.Removes.Length != 0)
        {
            throw InvalidDelta();
        }

        var batch = delta.Upserts.Span[0];
        var key = new ArcCadRenderBatchKey(batch.LayerId, batch.Color, batch.LinetypeId);
        if (lineBatchKey is { } existing && existing != key)
        {
            throw InvalidDelta();
        }

        var strips = batch.Strips.Span;
        var markerViews = batch.Markers.Span;
        var vertices = delta.Vertices.Span;
        if (strips.Length == 0 && markerViews.Length == 0 ||
            strips.Length == 0 && vertices.Length != 0 ||
            strips.Length > 0 && vertices.Length < 4 ||
            (vertices.Length & 1) != 0)
        {
            throw InvalidDelta();
        }

        foreach (var vertex in vertices)
        {
            if (!float.IsFinite(vertex))
            {
                throw InvalidDelta();
            }
        }

        var entities = new CadEntityPath[strips.Length];
        var ids = new HashSet<ulong>();
        var coveredPoints = new bool[vertices.Length / 2];
        for (var index = 0; index < strips.Length; index++)
        {
            var strip = strips[index];
            var pointEnd = (ulong)strip.Offset + strip.Count;
            if (strip.Count < 2 ||
                pointEnd > (ulong)coveredPoints.Length ||
                strip.EntityId == 0 ||
                !float.IsFinite(strip.Width) ||
                strip.Width < 0 ||
                !float.IsFinite(strip.PolyWidth) ||
                strip.PolyWidth < 0 ||
                strip.AnalyticLength is { } analyticLength &&
                    (!double.IsFinite(analyticLength) || analyticLength < 0) ||
                !ids.Add(strip.EntityId))
            {
                throw InvalidDelta();
            }

            var pointOffset = (int)strip.Offset;
            var pointCount = (int)strip.Count;
            for (var point = pointOffset; point < pointOffset + pointCount; point++)
            {
                if (coveredPoints[point])
                {
                    throw InvalidDelta();
                }

                coveredPoints[point] = true;
            }

            entities[index] = new CadEntityPath(
                strip.EntityId,
                vertices.Slice(pointOffset * 2, pointCount * 2).ToArray().AsMemory(),
                strip.PolyWidth,
                IsLine: false,
                AnalyticLength: strip.AnalyticLength);
        }

        if (coveredPoints.Any(covered => !covered))
        {
            throw InvalidDelta();
        }

        var markers = new CadMarker[markerViews.Length];
        for (var index = 0; index < markerViews.Length; index++)
        {
            var marker = markerViews[index];
            if (marker.EntityId == 0 ||
                !float.IsFinite(marker.X) ||
                !float.IsFinite(marker.Y) ||
                !ids.Add(marker.EntityId))
            {
                throw InvalidDelta();
            }

            markers[index] = new CadMarker(marker.EntityId, marker.X, marker.Y);
        }

        // ponytail: one native LIST batch per scene sync; carry type in render ABI if profiling requires it.
        var report = session.ListEntities(ids.ToArray());
        for (var index = 0; index < entities.Length; index++)
        {
            var entityType = ParseEntityType(report, entities[index].EntityId);
            entities[index] = entities[index] with
            {
                IsLine = entityType == "LINE",
                IsMask = entityType == "WIPEOUT",
            };
        }

        if (markers.Any(marker => ParseEntityType(report, marker.EntityId) != "POINT"))
        {
            throw InvalidDelta();
        }

        var lines = entities
            .Select(entity => entity.AsLine)
            .Where(line => line.HasValue)
            .Select(line => line!.Value)
            .ToArray();

        // ponytail: one default render batch; use a batch-keyed cache when layers/styles reach desktop.
        return (entities, markers, lines, key);
    }

    private static HashSet<ulong> SceneIds(CadEntityPath[] entities, CadMarker[] markers) =>
        entities.Select(entity => entity.EntityId)
            .Concat(markers.Select(marker => marker.EntityId))
            .ToHashSet();

    private static bool SceneContains(
        CadEntityPath[] entities,
        CadMarker[] markers,
        ulong entityId) =>
        entities.Any(entity => entity.EntityId == entityId) ||
        markers.Any(marker => marker.EntityId == entityId);

    private static bool ScenePreserved(
        CadEntityPath[] previousEntities,
        CadMarker[] previousMarkers,
        CadEntityPath[] nextEntities,
        CadMarker[] nextMarkers) =>
        previousEntities.All(previous =>
            nextEntities.Any(next => EntityPathEquals(previous, next))) &&
        previousMarkers.All(nextMarkers.Contains);

    private static bool SceneEquals(
        CadEntityPath[] firstEntities,
        CadMarker[] firstMarkers,
        CadEntityPath[] secondEntities,
        CadMarker[] secondMarkers) =>
        firstEntities.Length == secondEntities.Length &&
        firstMarkers.Length == secondMarkers.Length &&
        firstEntities.Zip(secondEntities).All(pair => EntityPathEquals(pair.First, pair.Second)) &&
        firstMarkers.AsSpan().SequenceEqual(secondMarkers);

    private static bool EntityPathEquals(CadEntityPath first, CadEntityPath second) =>
        first.EntityId == second.EntityId &&
        first.PolyWidth == second.PolyWidth &&
        first.AnalyticLength == second.AnalyticLength &&
        first.IsLine == second.IsLine &&
        first.IsMask == second.IsMask &&
        first.Vertices.Span.SequenceEqual(second.Vertices.Span);

    private static string ParseEntityType(string report, ulong entityId)
    {
        var marker = $" #{entityId}";
        var markerIndex = report.IndexOf(marker, StringComparison.Ordinal);
        var typeStart = markerIndex;
        while (typeStart > 0 && char.IsAsciiLetter(report[typeStart - 1]))
        {
            typeStart--;
        }

        if (typeStart == markerIndex)
        {
            throw new InvalidOperationException("Native LIST returned an invalid entity header.");
        }

        return report[typeStart..markerIndex];
    }

    private static InvalidOperationException InvalidDelta() =>
        new("Native session returned an invalid default geometry render delta.");
}
