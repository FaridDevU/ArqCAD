using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Runtime.ExceptionServices;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

[assembly: InternalsVisibleTo("ArcForge.Native.Smoke")]
[assembly: DefaultDllImportSearchPaths(
    DllImportSearchPath.AssemblyDirectory |
    DllImportSearchPath.UseDllDirectoryForDependencies)]

namespace ArcForge.Native;

internal enum AfStatus : uint
{
    Ok = 0,
    InvalidArgument = 1,
    InvalidHandle = 2,
    WrongThread = 3,
    Internal = 4,
    InvalidUtf8 = 5,
    Panic = 255,
}

[StructLayout(LayoutKind.Sequential)]
internal struct AfVersionNative
{
    internal ushort Major;
    internal ushort Minor;
    internal ushort Patch;

    internal AfVersionNative(ushort major, ushort minor, ushort patch)
    {
        Major = major;
        Minor = minor;
        Patch = patch;
    }
}

[StructLayout(LayoutKind.Sequential)]
internal struct AfUtf8BufferNative
{
    internal nint Data;
    internal nuint Length;
    internal nuint Capacity;
    internal nuint Owner;
}

[StructLayout(LayoutKind.Sequential)]
internal struct AfF32BufferNative
{
    internal nint Data;
    internal nuint Length;
    internal nuint Capacity;
    internal nuint Owner;
}

[StructLayout(LayoutKind.Sequential)]
internal struct AfByteBufferNative
{
    internal nint Data;
    internal nuint Length;
    internal nuint Capacity;
    internal nuint Owner;
}

internal static partial class AfNative
{
    private const string LibraryName = "af_ffi";

    static AfNative()
    {
        NativeLibrary.SetDllImportResolver(
            typeof(AfNative).Assembly,
            static (libraryName, _, _) =>
            {
                if (!string.Equals(libraryName, LibraryName, StringComparison.Ordinal))
                {
                    return nint.Zero;
                }

                var path = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "af_ffi.dll"));
                if (!File.Exists(path))
                {
                    throw new DllNotFoundException(
                        $"Required app-local native library was not found: {path}");
                }

                return NativeLibrary.Load(path);
            });
    }

    [LibraryImport(LibraryName, EntryPoint = "af_abi_version")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus AbiVersion(out AfVersionNative version);

    [LibraryImport(LibraryName, EntryPoint = "af_session_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionCreate(out nuint handle);

    [LibraryImport(LibraryName, EntryPoint = "af_session_destroy")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionDestroy(nuint handle);

    [LibraryImport(LibraryName, EntryPoint = "af_session_save_arcf")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionSaveArcf(
        AfSessionSafeHandle handle,
        out AfByteBufferNative bytes);

    [LibraryImport(LibraryName, EntryPoint = "af_session_open_arcf_json")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionOpenArcfJson(
        AfSessionSafeHandle handle,
        nint bytes,
        nuint bytesLength,
        out AfUtf8BufferNative result);

    [LibraryImport(LibraryName, EntryPoint = "af_session_execute_json")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionExecuteJson(
        AfSessionSafeHandle handle,
        nint command,
        nuint commandLength,
        nint argsJson,
        nuint argsJsonLength,
        out AfUtf8BufferNative result);

    [LibraryImport(LibraryName, EntryPoint = "af_session_layers_json")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionLayersJson(
        AfSessionSafeHandle handle,
        out AfUtf8BufferNative result);

    [LibraryImport(LibraryName, EntryPoint = "af_session_parse_input_json")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionParseInputJson(
        AfSessionSafeHandle handle,
        nint input,
        nuint inputLength,
        byte hasBase,
        double baseX,
        double baseY,
        out AfUtf8BufferNative result);

    [LibraryImport(LibraryName, EntryPoint = "af_session_snap_json")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionSnapJson(
        AfSessionSafeHandle handle,
        double x,
        double y,
        double radius,
        out AfUtf8BufferNative result);

    [LibraryImport(LibraryName, EntryPoint = "af_session_select_at_json")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionSelectAtJson(
        AfSessionSafeHandle handle,
        double x,
        double y,
        double tolerance,
        out AfUtf8BufferNative selection);

    [LibraryImport(LibraryName, EntryPoint = "af_session_render_delta")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionRenderDelta(
        AfSessionSafeHandle handle,
        out AfUtf8BufferNative control,
        out AfF32BufferNative vertices);

    [LibraryImport(LibraryName, EntryPoint = "af_session_render_full_json")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionRenderFullJson(
        AfSessionSafeHandle handle,
        out AfUtf8BufferNative result);

    [LibraryImport(LibraryName, EntryPoint = "af_session_render_vertices")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus SessionRenderVertices(
        AfSessionSafeHandle handle,
        out AfF32BufferNative vertices);

    [LibraryImport(LibraryName, EntryPoint = "af_utf8_buffer_free")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus Utf8BufferFree(ref AfUtf8BufferNative buffer);

    [LibraryImport(LibraryName, EntryPoint = "af_f32_buffer_free")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus F32BufferFree(ref AfF32BufferNative buffer);

    [LibraryImport(LibraryName, EntryPoint = "af_byte_buffer_free")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial AfStatus ByteBufferFree(ref AfByteBufferNative buffer);
}

internal static class AfNativeSessionWorker
{
    private static readonly BlockingCollection<IWorkItem> Queue = new();

    // ponytail: one native thread is enough until concurrent MDI profiling says otherwise.
    private static readonly Thread Worker = StartWorker();

    internal static T Invoke<T>(Func<T> operation)
    {
        ArgumentNullException.ThrowIfNull(operation);
        if (Environment.CurrentManagedThreadId == Worker.ManagedThreadId)
        {
            return operation();
        }

        var item = new WorkItem<T>(operation);
        Queue.Add(item);
        return item.GetResult();
    }

    private static Thread StartWorker()
    {
        var worker = new Thread(Run)
        {
            IsBackground = true,
            Name = "ArcForge.Native",
        };
        worker.Start();
        return worker;
    }

    private static void Run()
    {
        foreach (var item in Queue.GetConsumingEnumerable())
        {
            item.Execute();
        }
    }

    private interface IWorkItem
    {
        void Execute();
    }

    private sealed class WorkItem<T>(Func<T> operation) : IWorkItem
    {
        private readonly TaskCompletionSource<T> _completion =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        private Func<T>? _operation = operation;

        public void Execute()
        {
            T? result = default;
            Exception? error = null;
            try
            {
                result = _operation!();
            }
            catch (Exception exception)
            {
                error = exception;
            }
            finally
            {
                _operation = null;
                if (error is null)
                {
                    _completion.TrySetResult(result!);
                }
                else
                {
                    _completion.TrySetException(error);
                }
            }
        }

        internal T GetResult() => _completion.Task.GetAwaiter().GetResult();
    }
}

internal sealed class AfSessionSafeHandle : SafeHandle
{
    internal AfSessionSafeHandle(nuint value)
        : base(nint.Zero, ownsHandle: true)
    {
        SetHandle(unchecked((nint)value));
    }

    public override bool IsInvalid => handle == nint.Zero;

    internal nuint Value => unchecked((nuint)handle);

    internal void CloseChecked()
    {
        if (IsClosed || IsInvalid)
        {
            return;
        }

        var status = AfNativeSessionWorker.Invoke(() => AfNative.SessionDestroy(Value));
        if (status is not (AfStatus.Ok or AfStatus.InvalidHandle))
        {
            ArcCadSession.ThrowIfFailed(nameof(AfNative.SessionDestroy), status);
        }

        SetHandleAsInvalid();
    }

    protected override bool ReleaseHandle()
    {
        try
        {
            var status = AfNativeSessionWorker.Invoke(() => AfNative.SessionDestroy(Value));
            return status is AfStatus.Ok or AfStatus.InvalidHandle;
        }
        catch
        {
            return false;
        }
    }
}

public readonly record struct AfAbiVersion(ushort Major, ushort Minor, ushort Patch)
{
    public override string ToString() => $"{Major}.{Minor}.{Patch}";
}

public readonly record struct ArcCadPoint(double X, double Y);

public readonly record struct ArcCadSnap(
    ArcCadPoint Point,
    string Kind,
    ulong EntityId,
    double Distance);

public readonly record struct ArcCadRgba(byte Red, byte Green, byte Blue, byte Alpha);

public readonly record struct ArcCadRenderStrip(
    ulong EntityId,
    uint Offset,
    uint Count,
    float Width,
    float PolyWidth,
    double? AnalyticLength);

public readonly record struct ArcCadRenderMarker(ulong EntityId, float X, float Y);

public readonly record struct ArcCadRenderBatchKey(
    ulong LayerId,
    ArcCadRgba Color,
    ulong LinetypeId);

public sealed record ArcCadRenderBatch(
    ulong LayerId,
    ArcCadRgba Color,
    ulong LinetypeId,
    ReadOnlyMemory<ArcCadRenderStrip> Strips,
    ReadOnlyMemory<ArcCadRenderMarker> Markers);

public sealed record ArcCadRenderDelta(
    ReadOnlyMemory<ArcCadRenderBatch> Upserts,
    ReadOnlyMemory<ArcCadRenderBatchKey> Removes,
    ReadOnlyMemory<float> Vertices,
    double LinetypeScale);

public readonly record struct ArcCadLineResult(
    ulong TransactionSequence,
    ulong EntityId,
    string? Message);

public readonly record struct ArcCadMutationResult(
    ulong TransactionSequence,
    ReadOnlyMemory<ulong> CreatedEntityIds,
    string? Message);

public readonly record struct ArcCadLayerInfo(
    ulong Id,
    string Name,
    bool Off,
    bool Frozen,
    bool Locked,
    bool Plot,
    bool Current);

public sealed record ArcCadHistoryResult(string? Message);

public sealed class ArcCadCommandException : Exception
{
    internal ArcCadCommandException(string code, string message, string? detailJson)
        : base(message)
    {
        Code = code;
        DetailJson = detailJson;
    }

    public string Code { get; }

    public string? DetailJson { get; }
}

public sealed class ArcCadSession : IDisposable
{
    private const ushort SupportedAbiMajor = 0;
    private const ushort MinimumSupportedAbiMinor = 7;

    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    private readonly int _ownerThreadId;
    private readonly AfSessionSafeHandle _handle;

    private ArcCadSession(AfSessionSafeHandle handle, AfAbiVersion abiVersion)
    {
        _handle = handle;
        _ownerThreadId = Environment.CurrentManagedThreadId;
        AbiVersion = abiVersion;
    }

    public AfAbiVersion AbiVersion { get; }

    public bool IsDisposed => _handle.IsClosed || _handle.IsInvalid;

    internal nuint DangerousHandle => _handle.Value;

    internal AfSessionSafeHandle SafeHandle => _handle;

    public static ArcCadSession Create()
    {
        ThrowIfFailed(nameof(AfNative.AbiVersion), AfNative.AbiVersion(out var nativeVersion));
        var abiVersion = ValidateAbi(nativeVersion);

        nuint handle = 0;
        var status = AfNativeSessionWorker.Invoke(() => AfNative.SessionCreate(out handle));
        ThrowIfFailed(nameof(AfNative.SessionCreate), status);
        if (handle == 0)
        {
            throw new InvalidOperationException(
                $"{nameof(AfNative.SessionCreate)} returned native status 0 with an invalid handle.");
        }

        try
        {
            return new ArcCadSession(new AfSessionSafeHandle(handle), abiVersion);
        }
        catch
        {
            ThrowIfFailed(
                nameof(AfNative.SessionDestroy),
                AfNativeSessionWorker.Invoke(() => AfNative.SessionDestroy(handle)));
            throw;
        }
    }

    public ArcCadLineResult CreateLine(ArcCadPoint start, ArcCadPoint end)
    {
        ThrowIfPointIsNotFinite(start, nameof(start));
        ThrowIfPointIsNotFinite(end, nameof(end));

        var argsJson = JsonSerializer.Serialize(new
        {
            p1 = new[] { start.X, start.Y },
            p2 = new[] { end.X, end.Y },
        });

        return ParseLineResult(ExecuteJson("LINE", argsJson));
    }

    public ArcCadMutationResult CreatePoint(ArcCadPoint position)
    {
        ThrowIfPointIsNotFinite(position, nameof(position));
        return ExecuteMutation(
            "POINT",
            JsonSerializer.Serialize(new { position = new[] { position.X, position.Y } }));
    }

    public ArcCadMutationResult CreateXline(ArcCadPoint start, ArcCadPoint through)
    {
        ValidatePath([start, through], 2, "XLINE");
        return ExecuteMutation(
            "XLINE",
            JsonSerializer.Serialize(new
            {
                mode = "points",
                p1 = new[] { start.X, start.Y },
                p2 = new[] { through.X, through.Y },
            }));
    }

    public ArcCadMutationResult CreateHorizontalXline(ArcCadPoint point) =>
        CreateAxisXline(point, "hor");

    public ArcCadMutationResult CreateVerticalXline(ArcCadPoint point) =>
        CreateAxisXline(point, "ver");

    public ArcCadMutationResult CreateAngledXline(ArcCadPoint point, double angleRadians)
    {
        ThrowIfPointIsNotFinite(point, nameof(point));
        if (!double.IsFinite(angleRadians))
        {
            throw new ArgumentOutOfRangeException(nameof(angleRadians), "XLINE angle must be finite.");
        }

        return ExecuteMutation(
            "XLINE",
            JsonSerializer.Serialize(new
            {
                mode = "ang",
                p1 = new[] { point.X, point.Y },
                angle = angleRadians,
            }));
    }

    public ArcCadMutationResult CreateRay(ArcCadPoint origin, ArcCadPoint through)
    {
        ValidatePath([origin, through], 2, "RAY");
        return ExecuteMutation(
            "RAY",
            JsonSerializer.Serialize(new
            {
                origin = new[] { origin.X, origin.Y },
                through = new[] { through.X, through.Y },
            }));
    }

    public ArcCadMutationResult CreateDonut(
        ArcCadPoint center,
        double exteriorDiameter,
        double interiorDiameter)
    {
        ThrowIfPointIsNotFinite(center, nameof(center));
        if (!double.IsFinite(exteriorDiameter) || exteriorDiameter <= 0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(exteriorDiameter),
                "DONUT exterior diameter must be finite and positive.");
        }

        if (!double.IsFinite(interiorDiameter) ||
            interiorDiameter < 0 ||
            interiorDiameter >= exteriorDiameter)
        {
            throw new ArgumentOutOfRangeException(
                nameof(interiorDiameter),
                "DONUT interior diameter must be finite, non-negative and smaller than exterior diameter.");
        }

        var centerPoint = new[] { center.X, center.Y };
        var argsJson = interiorDiameter == 0
            ? JsonSerializer.Serialize(new { center = centerPoint, diam_ext = exteriorDiameter })
            : JsonSerializer.Serialize(new
            {
                center = centerPoint,
                diam_ext = exteriorDiameter,
                diam_int = interiorDiameter,
            });
        return ExecuteMutation("DONUT", argsJson);
    }

    public ArcCadMutationResult CreateCircle(ArcCadPoint center, double radius)
    {
        ThrowIfPointIsNotFinite(center, nameof(center));
        if (!double.IsFinite(radius) || radius <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(radius), "CIRCLE radius must be finite and positive.");
        }

        return ExecuteMutation(
            "CIRCLE",
            JsonSerializer.Serialize(new
            {
                mode = "center",
                center = new[] { center.X, center.Y },
                radius,
            }));
    }

    public ArcCadMutationResult CreateCircleTwoPoints(ArcCadPoint first, ArcCadPoint second)
    {
        ThrowIfPointIsNotFinite(first, nameof(first));
        ThrowIfPointIsNotFinite(second, nameof(second));
        if (first == second)
        {
            throw new ArgumentException("CIRCLE 2P needs two different points.", nameof(second));
        }

        return ExecuteMutation(
            "CIRCLE",
            JsonSerializer.Serialize(new
            {
                mode = "2p",
                p1 = new[] { first.X, first.Y },
                p2 = new[] { second.X, second.Y },
            }));
    }

    public ArcCadMutationResult CreateCircleThreePoints(
        ArcCadPoint first,
        ArcCadPoint second,
        ArcCadPoint third)
    {
        ThrowIfPointIsNotFinite(first, nameof(first));
        ThrowIfPointIsNotFinite(second, nameof(second));
        ThrowIfPointIsNotFinite(third, nameof(third));
        return ExecuteMutation(
            "CIRCLE",
            JsonSerializer.Serialize(new
            {
                mode = "3p",
                p1 = new[] { first.X, first.Y },
                p2 = new[] { second.X, second.Y },
                p3 = new[] { third.X, third.Y },
            }));
    }

    public ArcCadMutationResult CreateTangentCircle(
        ulong firstEntityId,
        ArcCadPoint firstPick,
        ulong secondEntityId,
        ArcCadPoint secondPick,
        double radius)
    {
        var entities = ValidateEntityIds([firstEntityId, secondEntityId]);
        if (entities.Length != 2 || firstEntityId == secondEntityId)
        {
            throw new ArgumentException("CIRCLE TTR needs two different entities.", nameof(secondEntityId));
        }

        ThrowIfPointIsNotFinite(firstPick, nameof(firstPick));
        ThrowIfPointIsNotFinite(secondPick, nameof(secondPick));
        if (!double.IsFinite(radius) || radius <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(radius), "CIRCLE TTR radius must be finite and positive.");
        }

        return ExecuteMutation(
            "CIRCLE",
            JsonSerializer.Serialize(new
            {
                mode = "ttr",
                entities,
                radius,
                p1 = new[] { firstPick.X, firstPick.Y },
                p2 = new[] { secondPick.X, secondPick.Y },
            }));
    }

    public ArcCadMutationResult CreateArc(
        ArcCadPoint start,
        ArcCadPoint middle,
        ArcCadPoint end)
    {
        ThrowIfPointIsNotFinite(start, nameof(start));
        ThrowIfPointIsNotFinite(middle, nameof(middle));
        ThrowIfPointIsNotFinite(end, nameof(end));
        return ExecuteMutation(
            "ARC",
            JsonSerializer.Serialize(new
            {
                mode = "3p",
                p1 = new[] { start.X, start.Y },
                p2 = new[] { middle.X, middle.Y },
                p3 = new[] { end.X, end.Y },
            }));
    }

    public ArcCadMutationResult CreateArcCenterStartEnd(
        ArcCadPoint center,
        ArcCadPoint start,
        ArcCadPoint end)
    {
        ThrowIfPointIsNotFinite(center, nameof(center));
        ThrowIfPointIsNotFinite(start, nameof(start));
        ThrowIfPointIsNotFinite(end, nameof(end));
        if (center == start)
        {
            throw new ArgumentException("ARC CSE needs a non-zero radius.", nameof(start));
        }

        return ExecuteMutation(
            "ARC",
            JsonSerializer.Serialize(new
            {
                mode = "cse",
                center = new[] { center.X, center.Y },
                start = new[] { start.X, start.Y },
                end = new[] { end.X, end.Y },
            }));
    }

    public ArcCadMutationResult CreateEllipse(
        ArcCadPoint center,
        ArcCadPoint axisEnd,
        double ratio)
    {
        ThrowIfPointIsNotFinite(center, nameof(center));
        ThrowIfPointIsNotFinite(axisEnd, nameof(axisEnd));
        if (Math.Abs(axisEnd.X - center.X) + Math.Abs(axisEnd.Y - center.Y) <= 0.000001)
        {
            throw new ArgumentException("ELLIPSE needs a non-zero major axis.", nameof(axisEnd));
        }

        if (!double.IsFinite(ratio) || ratio <= 0 || ratio > 1)
        {
            throw new ArgumentOutOfRangeException(nameof(ratio), "ELLIPSE ratio must be in (0, 1].");
        }

        return ExecuteMutation(
            "ELLIPSE",
            JsonSerializer.Serialize(new
            {
                mode = "center",
                center = new[] { center.X, center.Y },
                axisEnd = new[] { axisEnd.X, axisEnd.Y },
                ratio,
            }));
    }

    public ArcCadMutationResult CreateEllipticalArc(
        ArcCadPoint center,
        ArcCadPoint axisEnd,
        double ratio,
        double startParameterRadians,
        double endParameterRadians)
    {
        ThrowIfPointIsNotFinite(center, nameof(center));
        ThrowIfPointIsNotFinite(axisEnd, nameof(axisEnd));
        if (Math.Abs(axisEnd.X - center.X) + Math.Abs(axisEnd.Y - center.Y) <= 0.000001)
        {
            throw new ArgumentException("ELLIPSE ARC needs a non-zero major axis.", nameof(axisEnd));
        }

        if (!double.IsFinite(ratio) || ratio <= 0 || ratio > 1)
        {
            throw new ArgumentOutOfRangeException(nameof(ratio), "ELLIPSE ARC ratio must be in (0, 1].");
        }

        if (!double.IsFinite(startParameterRadians) || !double.IsFinite(endParameterRadians))
        {
            throw new ArgumentOutOfRangeException(nameof(startParameterRadians), "ELLIPSE ARC parameters must be finite.");
        }

        return ExecuteMutation(
            "ELLIPSE",
            JsonSerializer.Serialize(new
            {
                mode = "arc",
                center = new[] { center.X, center.Y },
                axisEnd = new[] { axisEnd.X, axisEnd.Y },
                ratio,
                startParam = startParameterRadians,
                endParam = endParameterRadians,
            }));
    }

    public ArcCadMutationResult CreateRectangle(
        ArcCadPoint first,
        ArcCadPoint opposite,
        double? chamfer1 = null,
        double? chamfer2 = null,
        double? fillet = null,
        double? width = null)
    {
        ThrowIfPointIsNotFinite(first, nameof(first));
        ThrowIfPointIsNotFinite(opposite, nameof(opposite));
        ValidateOptionalPositive(chamfer1, nameof(chamfer1));
        ValidateOptionalPositive(chamfer2, nameof(chamfer2));
        ValidateOptionalPositive(fillet, nameof(fillet));
        ValidateOptionalPositive(width, nameof(width));
        if (chamfer1.HasValue != chamfer2.HasValue)
        {
            throw new ArgumentException("RECTANG chamfer requires two distances.");
        }

        if (chamfer1.HasValue && fillet.HasValue)
        {
            throw new ArgumentException("RECTANG chamfer and fillet are mutually exclusive.");
        }

        var arguments = new Dictionary<string, object>
        {
            ["p1"] = new[] { first.X, first.Y },
            ["p2"] = new[] { opposite.X, opposite.Y },
        };
        if (chamfer1 is { } firstDistance && chamfer2 is { } secondDistance)
        {
            arguments["chamfer1"] = firstDistance;
            arguments["chamfer2"] = secondDistance;
        }

        if (fillet is { } radius)
        {
            arguments["fillet"] = radius;
        }

        if (width is { } polylineWidth)
        {
            arguments["width"] = polylineWidth;
        }

        return ExecuteMutation(
            "RECTANG",
            JsonSerializer.Serialize(arguments));
    }

    public ArcCadMutationResult CreatePolyline(IReadOnlyList<ArcCadPoint> points, bool closed = false)
    {
        ValidatePath(points, 2, "PLINE");

        return ExecuteMutation(
            "PLINE",
            JsonSerializer.Serialize(new
            {
                vertices = points.Select(point => new { pt = new[] { point.X, point.Y } }).ToArray(),
                closed,
            }));
    }

    public ArcCadMutationResult CreateSpline(IReadOnlyList<ArcCadPoint> points)
    {
        ValidatePath(points, 3, "SPLINE");
        return ExecuteMutation(
            "SPLINE",
            JsonSerializer.Serialize(new
            {
                points = points.Select(point => new { pt = new[] { point.X, point.Y } }).ToArray(),
                closed = false,
            }));
    }

    public ArcCadMutationResult CreateRevisionCloud(
        IReadOnlyList<ArcCadPoint> contour,
        double arcLength,
        string style = "NORMAL")
    {
        ValidatePath(contour, 3, "REVCLOUD");
        if (!double.IsFinite(arcLength) || arcLength <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(arcLength));
        }

        var normalizedStyle = NormalizeRevisionCloudStyle(style);

        return ExecuteMutation(
            "REVCLOUD",
            JsonSerializer.Serialize(new
            {
                contour = contour.Select(point => new { pt = new[] { point.X, point.Y } }).ToArray(),
                arc_len = arcLength,
                style = normalizedStyle,
            }));
    }

    public ArcCadMutationResult ConvertRevisionCloud(
        ulong sourceEntityId,
        double arcLength,
        string style = "NORMAL")
    {
        var source = ValidateEntityIds([sourceEntityId]);
        if (!double.IsFinite(arcLength) || arcLength <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(arcLength));
        }

        return ExecuteMutation(
            "REVCLOUD",
            JsonSerializer.Serialize(new
            {
                source,
                arc_len = arcLength,
                style = NormalizeRevisionCloudStyle(style),
            }));
    }

    public ArcCadMutationResult CreateWipeout(IReadOnlyList<ArcCadPoint> points)
    {
        ValidatePath(points, 3, "WIPEOUT");
        return ExecuteMutation(
            "WIPEOUT",
            JsonSerializer.Serialize(new
            {
                points = points.Select(point => new { pt = new[] { point.X, point.Y } }).ToArray(),
            }));
    }

    public ArcCadMutationResult CreatePolygon(
        ulong sides,
        ArcCadPoint center,
        double radius,
        bool circumscribed)
    {
        if (sides is < 3 or > 1024)
        {
            throw new ArgumentOutOfRangeException(nameof(sides), "POLYGON sides must be in 3..=1024.");
        }

        ThrowIfPointIsNotFinite(center, nameof(center));
        if (!double.IsFinite(radius) || radius <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(radius), "POLYGON radius must be finite and positive.");
        }

        return ExecuteMutation(
            "POLYGON",
            JsonSerializer.Serialize(new
            {
                sides,
                center = new[] { center.X, center.Y },
                radius,
                mode = circumscribed ? "circumscribed" : "inscribed",
                angle = 0.0,
            }));
    }

    public ArcCadMutationResult CreateLayer(string name)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(name);
        return ExecuteMutation("LAYER", JsonSerializer.Serialize(new { op = "new", name }));
    }

    public ArcCadMutationResult DeleteLayer(ulong layerId) =>
        ExecuteLayerOperation("delete", layerId);

    public ArcCadMutationResult RenameLayer(ulong layerId, string name)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(name);
        ValidateLayerId(layerId);
        return ExecuteMutation(
            "LAYER",
            JsonSerializer.Serialize(new { op = "rename", layer = layerId, name }));
    }

    public ArcCadMutationResult SetCurrentLayer(ulong layerId) =>
        ExecuteLayerOperation("set-current", layerId);

    public ArcCadMutationResult SetLayerOff(ulong layerId, bool value) =>
        ExecuteLayerOperation(value ? "off" : "on", layerId);

    public ArcCadMutationResult SetLayerFrozen(ulong layerId, bool value) =>
        ExecuteLayerOperation(value ? "freeze" : "thaw", layerId);

    public ArcCadMutationResult SetLayerLocked(ulong layerId, bool value) =>
        ExecuteLayerOperation(value ? "lock" : "unlock", layerId);

    public ArcCadMutationResult SetLayerPlot(ulong layerId, bool value) =>
        ExecuteLayerOperation(value ? "plot" : "no-plot", layerId);

    public ArcCadHistoryResult Undo() =>
        ParseHistoryResult(ExecuteJson("UNDO", "null"));

    public ArcCadHistoryResult Redo() =>
        ParseHistoryResult(ExecuteJson("REDO", "null"));

    public string ReinitializeAliases(string pgpContent)
    {
        ArgumentNullException.ThrowIfNull(pgpContent);
        var message = ParseHistoryResult(
            ExecuteJson(
                "__ARCFORGE_PGP_REINIT",
                JsonSerializer.Serialize(new { pgp = pgpContent }))).Message;
        return string.IsNullOrWhiteSpace(message)
            ? throw new InvalidOperationException("Native PGP reinit returned no result.")
            : message;
    }

    public string? ResolveAlias(string token)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(token);
        return ParseHistoryResult(
            ExecuteJson(
                "__ARCFORGE_PGP_RESOLVE",
                JsonSerializer.Serialize(new { token }))).Message;
    }

    public string IdentifyPoint(ArcCadPoint point)
    {
        ThrowIfPointIsNotFinite(point, nameof(point));
        return ExecuteReadOnlyQuery(
            "ID",
            JsonSerializer.Serialize(new { point = new[] { point.X, point.Y } }));
    }

    public string MeasureDistance(ArcCadPoint first, ArcCadPoint second)
    {
        ThrowIfPointIsNotFinite(first, nameof(first));
        ThrowIfPointIsNotFinite(second, nameof(second));
        return ExecuteReadOnlyQuery(
            "DIST",
            JsonSerializer.Serialize(new
            {
                p1 = new[] { first.X, first.Y },
                p2 = new[] { second.X, second.Y },
            }));
    }

    public string MeasureAngle(ArcCadPoint vertex, ArcCadPoint firstRay, ArcCadPoint secondRay)
    {
        ThrowIfPointIsNotFinite(vertex, nameof(vertex));
        ThrowIfPointIsNotFinite(firstRay, nameof(firstRay));
        ThrowIfPointIsNotFinite(secondRay, nameof(secondRay));
        return ExecuteReadOnlyQuery(
            "MEASUREGEOM",
            JsonSerializer.Serialize(new
            {
                mode = "angle",
                p1 = new[] { vertex.X, vertex.Y },
                p2 = new[] { firstRay.X, firstRay.Y },
                p3 = new[] { secondRay.X, secondRay.Y },
            }));
    }

    public string MeasureRadius(ReadOnlySpan<ulong> entityIds)
    {
        var entities = ValidateEntityIds(entityIds);
        return ExecuteReadOnlyQuery(
            "MEASUREGEOM",
            JsonSerializer.Serialize(new { mode = "radius", entities }));
    }

    public string MeasureLength(ReadOnlySpan<ulong> entityIds)
    {
        var entities = ValidateEntityIds(entityIds);
        return ExecuteReadOnlyQuery(
            "MEASUREGEOM",
            JsonSerializer.Serialize(new { mode = "length", entities }));
    }

    public string MeasureBounds(ReadOnlySpan<ulong> entityIds)
    {
        var entities = ValidateEntityIds(entityIds);
        return ExecuteReadOnlyQuery(
            "MEASUREGEOM",
            JsonSerializer.Serialize(new { mode = "bounds", entities }));
    }

    public string ListEntities(ReadOnlySpan<ulong> entityIds)
    {
        if (entityIds.IsEmpty)
        {
            throw new ArgumentException("At least one entity is required.", nameof(entityIds));
        }

        foreach (var entityId in entityIds)
        {
            if (entityId == 0)
            {
                throw new ArgumentException("Entity IDs must be non-zero.", nameof(entityIds));
            }
        }

        return ExecuteReadOnlyQuery(
            "LIST",
            JsonSerializer.Serialize(new { entities = entityIds.ToArray() }));
    }

    public string MeasureArea(ReadOnlySpan<ulong> entityIds)
    {
        var entities = ValidateEntityIds(entityIds);
        return ExecuteReadOnlyQuery(
            "AREA",
            JsonSerializer.Serialize(new { entities }));
    }

    public ArcCadMutationResult MoveEntities(
        ReadOnlySpan<ulong> entityIds,
        ArcCadPoint from,
        ArcCadPoint to)
    {
        var entities = ValidateEntityIds(entityIds);
        ThrowIfPointIsNotFinite(from, nameof(from));
        ThrowIfPointIsNotFinite(to, nameof(to));
        return ExecuteMutation(
            "MOVE",
            JsonSerializer.Serialize(new
            {
                entities,
                from = new[] { from.X, from.Y },
                to = new[] { to.X, to.Y },
            }));
    }

    public ArcCadMutationResult CopyEntities(
        ReadOnlySpan<ulong> entityIds,
        ArcCadPoint from,
        ArcCadPoint to)
    {
        var entities = ValidateEntityIds(entityIds);
        ThrowIfPointIsNotFinite(from, nameof(from));
        ThrowIfPointIsNotFinite(to, nameof(to));
        return ExecuteMutation(
            "COPY",
            JsonSerializer.Serialize(new
            {
                entities,
                from = new[] { from.X, from.Y },
                to = new[] { to.X, to.Y },
            }));
    }

    public ArcCadMutationResult MirrorEntities(
        ReadOnlySpan<ulong> entityIds,
        ArcCadPoint firstAxisPoint,
        ArcCadPoint secondAxisPoint,
        bool eraseSource = false)
    {
        var entities = ValidateEntityIds(entityIds);
        ThrowIfPointIsNotFinite(firstAxisPoint, nameof(firstAxisPoint));
        ThrowIfPointIsNotFinite(secondAxisPoint, nameof(secondAxisPoint));
        return ExecuteMutation(
            "MIRROR",
            JsonSerializer.Serialize(new
            {
                entities,
                p1 = new[] { firstAxisPoint.X, firstAxisPoint.Y },
                p2 = new[] { secondAxisPoint.X, secondAxisPoint.Y },
                erase_source = eraseSource,
            }));
    }

    public ArcCadMutationResult RotateEntities(
        ReadOnlySpan<ulong> entityIds,
        ArcCadPoint basePoint,
        double angleRadians)
    {
        var entities = ValidateEntityIds(entityIds);
        ThrowIfPointIsNotFinite(basePoint, nameof(basePoint));
        if (!double.IsFinite(angleRadians) || angleRadians == 0)
        {
            throw new ArgumentOutOfRangeException(nameof(angleRadians));
        }

        return ExecuteMutation(
            "ROTATE",
            JsonSerializer.Serialize(new
            {
                entities,
                @base = new[] { basePoint.X, basePoint.Y },
                angle = angleRadians,
            }));
    }

    public ArcCadMutationResult EraseEntities(ReadOnlySpan<ulong> entityIds)
    {
        var entities = ValidateEntityIds(entityIds);
        return ExecuteMutation("ERASE", JsonSerializer.Serialize(new { entities }));
    }

    public ArcCadMutationResult Oops() => ExecuteMutation("OOPS", "{}");

    public ArcCadMutationResult ExplodeEntities(ReadOnlySpan<ulong> entityIds)
    {
        var entities = ValidateEntityIds(entityIds);
        return ExecuteMutation("EXPLODE", JsonSerializer.Serialize(new { entities }));
    }

    public ArcCadMutationResult ScaleEntities(
        ReadOnlySpan<ulong> entityIds,
        ArcCadPoint basePoint,
        double factor)
    {
        var entities = ValidateEntityIds(entityIds);
        ThrowIfPointIsNotFinite(basePoint, nameof(basePoint));
        if (!double.IsFinite(factor) || factor <= 0 || factor == 1)
        {
            throw new ArgumentOutOfRangeException(nameof(factor));
        }

        return ExecuteMutation(
            "SCALE",
            JsonSerializer.Serialize(new
            {
                entities,
                @base = new[] { basePoint.X, basePoint.Y },
                factor,
            }));
    }

    public ArcCadMutationResult OffsetEntities(
        ReadOnlySpan<ulong> entityIds,
        double distance,
        ArcCadPoint side)
    {
        var entities = ValidateEntityIds(entityIds);
        if (!double.IsFinite(distance) || distance <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(distance));
        }

        ThrowIfPointIsNotFinite(side, nameof(side));
        return ExecuteMutation(
            "OFFSET",
            JsonSerializer.Serialize(new
            {
                entities,
                distance,
                side = new[] { side.X, side.Y },
            }));
    }

    public ArcCadMutationResult TrimEntities(
        ReadOnlySpan<ulong> targetEntityIds,
        ReadOnlySpan<ulong> edgeEntityIds,
        ArcCadPoint pick) => ExecuteEdgeMutation("TRIM", targetEntityIds, edgeEntityIds, pick);

    public ArcCadMutationResult ExtendEntities(
        ReadOnlySpan<ulong> targetEntityIds,
        ReadOnlySpan<ulong> edgeEntityIds,
        ArcCadPoint pick) => ExecuteEdgeMutation("EXTEND", targetEntityIds, edgeEntityIds, pick);

    public ArcCadMutationResult ChamferEntities(
        ReadOnlySpan<ulong> entityIds,
        double firstDistance,
        double secondDistance)
    {
        var entities = ValidateEntityIds(entityIds);
        if (entities.Length != 2 || !double.IsFinite(firstDistance) || firstDistance <= 0 ||
            !double.IsFinite(secondDistance) || secondDistance <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(firstDistance));
        }

        return ExecuteMutation(
            "CHAMFER",
            JsonSerializer.Serialize(new { entities, d1 = firstDistance, d2 = secondDistance }));
    }

    public ArcCadMutationResult FilletEntities(ReadOnlySpan<ulong> entityIds, double radius)
    {
        var entities = ValidateEntityIds(entityIds);
        if (entities.Length != 2)
        {
            throw new ArgumentException("FILLET needs exactly two entities.", nameof(entityIds));
        }

        if (!double.IsFinite(radius) || radius <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(radius));
        }

        return ExecuteMutation(
            "FILLET",
            JsonSerializer.Serialize(new { entities, radius }));
    }

    public ArcCadMutationResult BreakEntity(ulong entityId, ArcCadPoint first, ArcCadPoint second)
    {
        var target = ValidateEntityIds([entityId]);
        ThrowIfPointIsNotFinite(first, nameof(first));
        ThrowIfPointIsNotFinite(second, nameof(second));
        if (first == second)
        {
            throw new ArgumentException("BREAK needs two different points.", nameof(second));
        }

        return ExecuteMutation(
            "BREAK",
            JsonSerializer.Serialize(new
            {
                target,
                p1 = new[] { first.X, first.Y },
                p2 = new[] { second.X, second.Y },
            }));
    }

    public ArcCadMutationResult BreakEntityAtPoint(ulong entityId, ArcCadPoint point)
    {
        var target = ValidateEntityIds([entityId]);
        ThrowIfPointIsNotFinite(point, nameof(point));
        return ExecuteMutation(
            "BREAKATPOINT",
            JsonSerializer.Serialize(new { target, point = new[] { point.X, point.Y } }));
    }

    public ArcCadMutationResult LengthenEntity(ulong entityId, ArcCadPoint pick, double total)
    {
        var target = ValidateEntityIds([entityId]);
        ThrowIfPointIsNotFinite(pick, nameof(pick));
        if (!double.IsFinite(total) || total <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(total));
        }

        return ExecuteMutation(
            "LENGTHEN",
            JsonSerializer.Serialize(new
            {
                target,
                pick = new[] { pick.X, pick.Y },
                total,
            }));
    }

    public ArcCadMutationResult StretchEntities(
        ReadOnlySpan<ulong> entityIds,
        ArcCadPoint firstCorner,
        ArcCadPoint secondCorner,
        ArcCadPoint basePoint,
        ArcCadPoint destination)
    {
        var entities = ValidateEntityIds(entityIds);
        ThrowIfPointIsNotFinite(firstCorner, nameof(firstCorner));
        ThrowIfPointIsNotFinite(secondCorner, nameof(secondCorner));
        ThrowIfPointIsNotFinite(basePoint, nameof(basePoint));
        ThrowIfPointIsNotFinite(destination, nameof(destination));
        if (firstCorner == secondCorner || basePoint == destination)
        {
            throw new ArgumentException("STRETCH needs a window and a non-zero displacement.");
        }

        return ExecuteMutation(
            "STRETCH",
            JsonSerializer.Serialize(new
            {
                entities,
                corner1 = new[] { firstCorner.X, firstCorner.Y },
                corner2 = new[] { secondCorner.X, secondCorner.Y },
                @base = new[] { basePoint.X, basePoint.Y },
                to = new[] { destination.X, destination.Y },
            }));
    }

    public ArcCadMutationResult JoinEntities(ReadOnlySpan<ulong> entityIds)
    {
        var entities = ValidateEntityIds(entityIds);
        if (entities.Length < 2)
        {
            throw new ArgumentException("JOIN needs at least two entities.", nameof(entityIds));
        }

        return ExecuteMutation("JOIN", JsonSerializer.Serialize(new { entities }));
    }

    public ArcCadMutationResult AlignEntities(
        ReadOnlySpan<ulong> entityIds,
        ArcCadPoint firstSource,
        ArcCadPoint firstDestination,
        ArcCadPoint secondSource,
        ArcCadPoint secondDestination)
    {
        var entities = ValidateEntityIds(entityIds);
        ThrowIfPointIsNotFinite(firstSource, nameof(firstSource));
        ThrowIfPointIsNotFinite(firstDestination, nameof(firstDestination));
        ThrowIfPointIsNotFinite(secondSource, nameof(secondSource));
        ThrowIfPointIsNotFinite(secondDestination, nameof(secondDestination));
        if (firstSource == secondSource || firstDestination == secondDestination)
        {
            throw new ArgumentException("ALIGN needs two source and destination directions.");
        }

        return ExecuteMutation(
            "ALIGN",
            JsonSerializer.Serialize(new
            {
                entities,
                src1 = new[] { firstSource.X, firstSource.Y },
                dst1 = new[] { firstDestination.X, firstDestination.Y },
                src2 = new[] { secondSource.X, secondSource.Y },
                dst2 = new[] { secondDestination.X, secondDestination.Y },
            }));
    }

    public ArcCadMutationResult NudgeEntities(ReadOnlySpan<ulong> entityIds, ArcCadPoint delta)
    {
        var entities = ValidateEntityIds(entityIds);
        ThrowIfPointIsNotFinite(delta, nameof(delta));
        if (delta.X == 0 && delta.Y == 0)
        {
            throw new ArgumentException("NUDGE needs a non-zero vector.", nameof(delta));
        }

        return ExecuteMutation(
            "NUDGE",
            JsonSerializer.Serialize(new { entities, delta = new[] { delta.X, delta.Y } }));
    }

    public ArcCadMutationResult? OverkillEntities(ReadOnlySpan<ulong> entityIds)
    {
        var entities = ValidateEntityIds(entityIds);
        return ParseOptionalMutationResult(
            ExecuteJson("OVERKILL", JsonSerializer.Serialize(new { entities })));
    }

    private ArcCadMutationResult ExecuteEdgeMutation(
        string command,
        ReadOnlySpan<ulong> targetEntityIds,
        ReadOnlySpan<ulong> edgeEntityIds,
        ArcCadPoint pick)
    {
        var target = ValidateEntityIds(targetEntityIds);
        var edges = edgeEntityIds.ToArray();
        if (edges.Any(entityId => entityId == 0) || edges.Distinct().Count() != edges.Length)
        {
            throw new ArgumentException("Edge IDs must be unique and non-zero.", nameof(edgeEntityIds));
        }

        ThrowIfPointIsNotFinite(pick, nameof(pick));
        return ExecuteMutation(
            command,
            JsonSerializer.Serialize(new
            {
                edges,
                target,
                pick = new[] { pick.X, pick.Y },
            }));
    }

    public ArcCadMutationResult CreateRectangularArray(
        ReadOnlySpan<ulong> entityIds,
        ulong rows,
        ulong columns,
        ArcCadPoint spacing)
    {
        var entities = ValidateEntityIds(entityIds);
        if (rows == 0 || columns == 0 || rows > ulong.MaxValue / columns || rows * columns < 2)
        {
            throw new ArgumentOutOfRangeException(nameof(rows), "The array must contain at least two cells.");
        }

        ThrowIfPointIsNotFinite(spacing, nameof(spacing));
        return ExecuteMutation(
            "ARRAY",
            JsonSerializer.Serialize(new
            {
                entities,
                mode = "rect",
                rows,
                cols = columns,
                spacing = new[] { spacing.X, spacing.Y },
            }));
    }

    private ArcCadMutationResult ExecuteMutation(string command, string argsJson) =>
        ParseMutationResult(ExecuteJson(command, argsJson));

    private ArcCadMutationResult ExecuteLayerOperation(string operation, ulong layerId)
    {
        ValidateLayerId(layerId);
        return ExecuteMutation(
            "LAYER",
            JsonSerializer.Serialize(new { op = operation, layer = layerId }));
    }

    private string ExecuteReadOnlyQuery(string command, string argsJson)
    {
        var message = ParseHistoryResult(ExecuteJson(command, argsJson)).Message;
        return string.IsNullOrWhiteSpace(message)
            ? throw new InvalidOperationException($"Native {command} returned no query result.")
            : message;
    }

    private static ulong[] ValidateEntityIds(ReadOnlySpan<ulong> entityIds)
    {
        if (entityIds.IsEmpty)
        {
            throw new ArgumentException("At least one entity is required.", nameof(entityIds));
        }

        var entities = entityIds.ToArray();
        if (entities.Any(entityId => entityId == 0) || entities.Distinct().Count() != entities.Length)
        {
            throw new ArgumentException("Entity IDs must be unique and non-zero.", nameof(entityIds));
        }

        return entities;
    }

    private static void ValidateLayerId(ulong layerId)
    {
        if (layerId == 0)
        {
            throw new ArgumentOutOfRangeException(nameof(layerId));
        }
    }

    public byte[] SaveArcf()
    {
        ThrowIfUnavailable();
        var (status, bytes) = AfNativeSessionWorker.Invoke(() =>
        {
            var callStatus = AfNative.SessionSaveArcf(_handle, out var callBytes);
            return (callStatus, callBytes);
        });
        return CopyAndFreeByteResult(
            nameof(AfNative.SessionSaveArcf),
            status,
            ref bytes);
    }

    public unsafe IReadOnlyList<string> OpenArcf(ReadOnlySpan<byte> bytes)
    {
        ThrowIfUnavailable();
        AfStatus status;
        AfUtf8BufferNative result;
        var bytesLength = (nuint)bytes.Length;
        fixed (byte* bytesPointer = bytes)
        {
            var address = bytes.IsEmpty ? nint.Zero : (nint)bytesPointer;
            var invocation = AfNativeSessionWorker.Invoke(() =>
            {
                var callStatus = AfNative.SessionOpenArcfJson(
                    _handle,
                    address,
                    bytesLength,
                    out var callResult);
                return (callStatus, callResult);
            });
            status = invocation.callStatus;
            result = invocation.callResult;
        }

        return ParseOpenArcfResult(CopyAndFreeUtf8Result(
            nameof(AfNative.SessionOpenArcfJson),
            status,
            ref result));
    }

    public unsafe ArcCadPoint ParsePoint(string input, ArcCadPoint? basePoint = null)
    {
        ArgumentNullException.ThrowIfNull(input);
        if (basePoint is { } point)
        {
            ThrowIfPointIsNotFinite(point, nameof(basePoint));
        }

        ThrowIfUnavailable();
        var inputBytes = StrictUtf8.GetBytes(input);
        AfStatus status;
        AfUtf8BufferNative result;
        fixed (byte* inputPointer = inputBytes)
        {
            var address = inputBytes.Length == 0 ? nint.Zero : (nint)inputPointer;
            var invocation = AfNativeSessionWorker.Invoke(() =>
            {
                var callStatus = AfNative.SessionParseInputJson(
                    _handle,
                    address,
                    (nuint)inputBytes.Length,
                    basePoint.HasValue ? (byte)1 : (byte)0,
                    basePoint?.X ?? 0.0,
                    basePoint?.Y ?? 0.0,
                    out var callResult);
                return (callStatus, callResult);
            });
            status = invocation.callStatus;
            result = invocation.callResult;
        }

        return ParsePointResult(CopyAndFreeUtf8Result(
            nameof(AfNative.SessionParseInputJson),
            status,
            ref result));
    }

    public IReadOnlyList<ArcCadSnap> Snap(ArcCadPoint cursor, double radius)
    {
        ThrowIfPointIsNotFinite(cursor, nameof(cursor));
        if (!double.IsFinite(radius) || radius <= 0.0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(radius),
                radius,
                "Snap radius must be finite and greater than zero.");
        }

        ThrowIfUnavailable();
        var (status, result) = AfNativeSessionWorker.Invoke(() =>
        {
            var callStatus = AfNative.SessionSnapJson(
                _handle,
                cursor.X,
                cursor.Y,
                radius,
                out var callResult);
            return (callStatus, callResult);
        });
        return ParseSnaps(CopyAndFreeUtf8Result(
            nameof(AfNative.SessionSnapJson),
            status,
            ref result));
    }

    public IReadOnlyList<ulong> SelectAt(ArcCadPoint point, double tolerance)
    {
        ThrowIfPointIsNotFinite(point, nameof(point));
        if (!double.IsFinite(tolerance) || tolerance <= 0.0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(tolerance),
                tolerance,
                "Selection tolerance must be finite and greater than zero.");
        }

        ThrowIfUnavailable();
        var (status, selection) = AfNativeSessionWorker.Invoke(() =>
        {
            var callStatus = AfNative.SessionSelectAtJson(
                _handle,
                point.X,
                point.Y,
                tolerance,
                out var callSelection);
            return (callStatus, callSelection);
        });
        return ParseSelection(CopyAndFreeUtf8Result(
            nameof(AfNative.SessionSelectAtJson),
            status,
            ref selection));
    }

    public ArcCadRenderDelta RenderDelta()
    {
        ThrowIfUnavailable();
        var (status, control, vertices) = AfNativeSessionWorker.Invoke(() =>
        {
            var callStatus = AfNative.SessionRenderDelta(
                _handle,
                out var callControl,
                out var callVertices);
            return (callStatus, callControl, callVertices);
        });
        return CopyParseAndFreeRenderDelta(status, ref control, ref vertices);
    }

    public ArcCadRenderDelta RenderFull() => ParseRenderFull(RenderFullJson());

    public IReadOnlyList<ArcCadLayerInfo> Layers()
    {
        ThrowIfUnavailable();
        var (status, result) = AfNativeSessionWorker.Invoke(() =>
        {
            var callStatus = AfNative.SessionLayersJson(_handle, out var callResult);
            return (callStatus, callResult);
        });
        return ParseLayers(CopyAndFreeUtf8Result(
            nameof(AfNative.SessionLayersJson),
            status,
            ref result));
    }

    public unsafe string ExecuteJson(string command, string argsJson)
    {
        ArgumentNullException.ThrowIfNull(command);
        ArgumentNullException.ThrowIfNull(argsJson);

        if (IsDisposed)
        {
            throw new ObjectDisposedException(nameof(ArcCadSession));
        }

        ThrowIfWrongThread();

        var commandBytes = StrictUtf8.GetBytes(command);
        var argsJsonBytes = StrictUtf8.GetBytes(argsJson);
        AfStatus status;
        AfUtf8BufferNative result;

        fixed (byte* commandPointer = commandBytes)
        fixed (byte* argsJsonPointer = argsJsonBytes)
        {
            var commandAddress = commandBytes.Length == 0 ? nint.Zero : (nint)commandPointer;
            var argsJsonAddress = argsJsonBytes.Length == 0 ? nint.Zero : (nint)argsJsonPointer;
            var invocation = AfNativeSessionWorker.Invoke(() =>
            {
                var callStatus = AfNative.SessionExecuteJson(
                    _handle,
                    commandAddress,
                    (nuint)commandBytes.Length,
                    argsJsonAddress,
                    (nuint)argsJsonBytes.Length,
                    out var callResult);
                return (callStatus, callResult);
            });
            status = invocation.callStatus;
            result = invocation.callResult;
        }

        return CopyAndFreeUtf8Result(
            nameof(AfNative.SessionExecuteJson),
            status,
            ref result);
    }

    public string RenderFullJson()
    {
        if (IsDisposed)
        {
            throw new ObjectDisposedException(nameof(ArcCadSession));
        }

        ThrowIfWrongThread();

        var (status, result) = AfNativeSessionWorker.Invoke(() =>
        {
            var callStatus = AfNative.SessionRenderFullJson(_handle, out var callResult);
            return (callStatus, callResult);
        });
        return CopyAndFreeUtf8Result(
            nameof(AfNative.SessionRenderFullJson),
            status,
            ref result);
    }

    public float[] RenderVertices()
    {
        if (IsDisposed)
        {
            throw new ObjectDisposedException(nameof(ArcCadSession));
        }

        ThrowIfWrongThread();

        var (status, vertices) = AfNativeSessionWorker.Invoke(() =>
        {
            var callStatus = AfNative.SessionRenderVertices(_handle, out var callVertices);
            return (callStatus, callVertices);
        });
        return CopyAndFreeF32Result(
            nameof(AfNative.SessionRenderVertices),
            status,
            ref vertices);
    }

    public void Dispose()
    {
        if (IsDisposed)
        {
            return;
        }

        ThrowIfWrongThread();

        _handle.CloseChecked();
    }

    internal static AfAbiVersion ValidateAbi(AfVersionNative version)
    {
        if (version.Major != SupportedAbiMajor || version.Minor < MinimumSupportedAbiMinor)
        {
            throw new NotSupportedException(
                $"Unsupported af-ffi ABI {version.Major}.{version.Minor}.{version.Patch}; " +
                $"expected {SupportedAbiMajor}.{MinimumSupportedAbiMinor}.x or a later compatible minor.");
        }

        return new AfAbiVersion(version.Major, version.Minor, version.Patch);
    }

    internal static void ThrowIfFailed(string operation, AfStatus status)
    {
        if (status != AfStatus.Ok)
        {
            throw new InvalidOperationException(
                $"{operation} failed with native status {(uint)status}.");
        }
    }

    internal static ArcCadLineResult ParseLineResult(string json)
    {
        ArgumentNullException.ThrowIfNull(json);

        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object)
            {
                throw InvalidLineEnvelope();
            }

            var hasOk = root.TryGetProperty("ok", out var ok);
            var hasError = root.TryGetProperty("error", out var error);
            if (hasOk == hasError)
            {
                throw InvalidLineEnvelope();
            }

            if (hasError)
            {
                throw ParseCommandError(
                    error,
                    "af_session_execute_json returned an invalid LINE envelope.");
            }

            return ParseLineSuccess(ok);
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(
                "af_session_execute_json returned invalid LINE JSON.",
                exception);
        }
    }

    internal static ArcCadMutationResult ParseMutationResult(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        const string invalid = "af_session_execute_json returned an invalid mutation envelope.";
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object || root.EnumerateObject().Count() != 1)
            {
                throw new InvalidOperationException(invalid);
            }

            var hasOk = root.TryGetProperty("ok", out var ok);
            var hasError = root.TryGetProperty("error", out var error);
            if (hasOk == hasError)
            {
                throw new InvalidOperationException(invalid);
            }

            if (hasError)
            {
                throw ParseCommandError(error, invalid);
            }

            return ParseMutationSuccess(ok, invalid);
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(
                "af_session_execute_json returned invalid mutation JSON.",
                exception);
        }
    }

    internal static ArcCadMutationResult? ParseOptionalMutationResult(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind == JsonValueKind.Object &&
                root.TryGetProperty("ok", out var ok) &&
                ok.ValueKind == JsonValueKind.Object &&
                ok.TryGetProperty("txSeq", out var transactionSequence) &&
                transactionSequence.ValueKind == JsonValueKind.Null)
            {
                _ = ParseHistoryResult(json);
                return null;
            }
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(
                "af_session_execute_json returned invalid optional mutation JSON.",
                exception);
        }

        return ParseMutationResult(json);
    }

    internal static IReadOnlyList<ulong> ParseSelection(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        const string invalid = "af_session_select_at_json returned invalid selection JSON.";
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Array)
            {
                throw new InvalidOperationException(invalid);
            }

            var selection = new ulong[root.GetArrayLength()];
            var seen = new HashSet<ulong>();
            var index = 0;
            foreach (var item in root.EnumerateArray())
            {
                if (item.ValueKind != JsonValueKind.Number ||
                    !item.TryGetUInt64(out var entityId) ||
                    entityId == 0 ||
                    !seen.Add(entityId))
                {
                    throw new InvalidOperationException(invalid);
                }

                selection[index++] = entityId;
            }

            return Array.AsReadOnly(selection);
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(invalid, exception);
        }
    }

    internal static ArcCadHistoryResult ParseHistoryResult(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        const string invalid = "af_session_execute_json returned an invalid UNDO/REDO envelope.";
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object)
            {
                throw new InvalidOperationException(invalid);
            }

            var propertyCount = 0;
            foreach (var _ in root.EnumerateObject())
            {
                propertyCount++;
            }

            var hasOk = root.TryGetProperty("ok", out var ok);
            var hasError = root.TryGetProperty("error", out var error);
            if (propertyCount != 1 || hasOk == hasError)
            {
                throw new InvalidOperationException(invalid);
            }

            if (hasError)
            {
                throw ParseCommandError(error, invalid);
            }

            return ParseHistorySuccess(ok, invalid);
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(
                "af_session_execute_json returned invalid UNDO/REDO JSON.",
                exception);
        }
    }

    internal static IReadOnlyList<string> ParseOpenArcfResult(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        const string invalid = "af_session_open_arcf_json returned an invalid open envelope.";
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object)
            {
                throw new InvalidOperationException(invalid);
            }

            var propertyCount = 0;
            foreach (var _ in root.EnumerateObject())
            {
                propertyCount++;
            }

            var hasOk = root.TryGetProperty("ok", out var ok);
            var hasError = root.TryGetProperty("error", out var error);
            if (propertyCount != 1 || hasOk == hasError)
            {
                throw new InvalidOperationException(invalid);
            }

            if (hasError)
            {
                throw ParseOpenArcfError(error, invalid);
            }

            if (ok.ValueKind != JsonValueKind.Array)
            {
                throw new InvalidOperationException(invalid);
            }

            var warnings = new string[ok.GetArrayLength()];
            var index = 0;
            foreach (var warning in ok.EnumerateArray())
            {
                if (warning.ValueKind != JsonValueKind.String)
                {
                    throw new InvalidOperationException(invalid);
                }

                warnings[index++] = warning.GetString()!;
            }

            return Array.AsReadOnly(warnings);
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(invalid, exception);
        }
    }

    internal static ArcCadPoint ParsePointResult(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        const string invalid = "af_session_parse_input_json returned invalid point JSON.";
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object)
            {
                throw new InvalidOperationException(invalid);
            }

            var hasOk = root.TryGetProperty("ok", out var ok);
            var hasError = root.TryGetProperty("error", out var error);
            if (hasOk == hasError)
            {
                throw new InvalidOperationException(invalid);
            }

            if (hasError)
            {
                throw ParseCommandError(error, invalid);
            }

            if (ok.ValueKind != JsonValueKind.Object ||
                !ok.TryGetProperty("point", out var point))
            {
                throw new InvalidOperationException(invalid);
            }

            return ParsePointArray(point, invalid);
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(invalid, exception);
        }
    }

    internal static IReadOnlyList<ArcCadSnap> ParseSnaps(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        const string invalid = "af_session_snap_json returned invalid snap JSON.";
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Array)
            {
                throw new InvalidOperationException(invalid);
            }

            var snaps = new ArcCadSnap[root.GetArrayLength()];
            var index = 0;
            foreach (var item in root.EnumerateArray())
            {
                if (item.ValueKind != JsonValueKind.Object ||
                    !item.TryGetProperty("point", out var point) ||
                    !item.TryGetProperty("kind", out var kindElement) ||
                    kindElement.ValueKind != JsonValueKind.String ||
                    !item.TryGetProperty("entity", out var entityElement) ||
                    !entityElement.TryGetUInt64(out var entityId) ||
                    entityId == 0 ||
                    !item.TryGetProperty("dist", out var distanceElement) ||
                    !distanceElement.TryGetDouble(out var distance) ||
                    !double.IsFinite(distance) ||
                    distance < 0.0)
                {
                    throw new InvalidOperationException(invalid);
                }

                var kind = kindElement.GetString();
                if (string.IsNullOrWhiteSpace(kind))
                {
                    throw new InvalidOperationException(invalid);
                }

                snaps[index++] = new ArcCadSnap(
                    ParsePointArray(point, invalid),
                    kind,
                    entityId,
                    distance);
            }

            return Array.AsReadOnly(snaps);
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(invalid, exception);
        }
    }

    internal static ArcCadRenderDelta ParseRenderDelta(string json, float[] vertices)
    {
        ArgumentNullException.ThrowIfNull(json);
        ArgumentNullException.ThrowIfNull(vertices);
        const string invalid = "af_session_render_delta returned invalid delta JSON.";
        if ((vertices.Length & 1) != 0 || Array.Exists(vertices, value => !float.IsFinite(value)))
        {
            throw new InvalidOperationException(invalid);
        }

        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object ||
                !root.TryGetProperty("upserts", out var upsertsElement) ||
                upsertsElement.ValueKind != JsonValueKind.Array ||
                !root.TryGetProperty("removes", out var removesElement) ||
                removesElement.ValueKind != JsonValueKind.Array ||
                !root.TryGetProperty("vertices", out var controlVertices) ||
                controlVertices.ValueKind != JsonValueKind.Array ||
                controlVertices.GetArrayLength() != 0 ||
                !root.TryGetProperty("ltscale", out var linetypeScaleElement) ||
                !linetypeScaleElement.TryGetDouble(out var linetypeScale) ||
                !double.IsFinite(linetypeScale) ||
                linetypeScale <= 0.0)
            {
                throw new InvalidOperationException(invalid);
            }

            var upserts = new ArcCadRenderBatch[upsertsElement.GetArrayLength()];
            var index = 0;
            foreach (var item in upsertsElement.EnumerateArray())
            {
                upserts[index++] = ParseRenderBatch(item, vertices.Length, invalid);
            }

            var removes = new ArcCadRenderBatchKey[removesElement.GetArrayLength()];
            index = 0;
            foreach (var item in removesElement.EnumerateArray())
            {
                removes[index++] = ParseRenderBatchKey(item, invalid);
            }

            return new ArcCadRenderDelta(
                upserts.AsMemory(),
                removes.AsMemory(),
                vertices.AsMemory(),
                linetypeScale);
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(invalid, exception);
        }
    }

    internal static ArcCadRenderDelta ParseRenderFull(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        const string invalid = "af_session_render_full_json returned invalid render JSON.";
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object ||
                !root.TryGetProperty("batches", out var batchesElement) ||
                batchesElement.ValueKind != JsonValueKind.Array ||
                !root.TryGetProperty("vertices", out var verticesElement) ||
                verticesElement.ValueKind != JsonValueKind.Array ||
                (verticesElement.GetArrayLength() & 1) != 0 ||
                !root.TryGetProperty("ltscale", out var linetypeScaleElement) ||
                !linetypeScaleElement.TryGetDouble(out var linetypeScale) ||
                !double.IsFinite(linetypeScale) ||
                linetypeScale <= 0.0)
            {
                throw new InvalidOperationException(invalid);
            }

            var vertices = new float[verticesElement.GetArrayLength()];
            var index = 0;
            foreach (var item in verticesElement.EnumerateArray())
            {
                if (!item.TryGetSingle(out var value) || !float.IsFinite(value))
                {
                    throw new InvalidOperationException(invalid);
                }

                vertices[index++] = value;
            }

            var batches = new ArcCadRenderBatch[batchesElement.GetArrayLength()];
            index = 0;
            foreach (var item in batchesElement.EnumerateArray())
            {
                batches[index++] = ParseRenderBatch(item, vertices.Length, invalid);
            }

            return new ArcCadRenderDelta(
                batches.AsMemory(),
                ReadOnlyMemory<ArcCadRenderBatchKey>.Empty,
                vertices.AsMemory(),
                linetypeScale);
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(invalid, exception);
        }
    }

    internal static IReadOnlyList<ArcCadLayerInfo> ParseLayers(string json)
    {
        ArgumentNullException.ThrowIfNull(json);
        const string invalid = "af_session_layers_json returned invalid layer JSON.";
        try
        {
            using var document = JsonDocument.Parse(json);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Array || root.GetArrayLength() == 0)
            {
                throw new InvalidOperationException(invalid);
            }

            var layers = new ArcCadLayerInfo[root.GetArrayLength()];
            var ids = new HashSet<ulong>();
            var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            var currentCount = 0;
            var index = 0;
            foreach (var item in root.EnumerateArray())
            {
                if (item.ValueKind != JsonValueKind.Object ||
                    !item.TryGetProperty("id", out var idElement) ||
                    !idElement.TryGetUInt64(out var id) || id == 0 ||
                    !item.TryGetProperty("name", out var nameElement) ||
                    nameElement.ValueKind != JsonValueKind.String ||
                    string.IsNullOrWhiteSpace(nameElement.GetString()) ||
                    !TryGetBoolean(item, "off", out var off) ||
                    !TryGetBoolean(item, "frozen", out var frozen) ||
                    !TryGetBoolean(item, "locked", out var locked) ||
                    !TryGetBoolean(item, "plot", out var plot) ||
                    !TryGetBoolean(item, "current", out var current))
                {
                    throw new InvalidOperationException(invalid);
                }

                var name = nameElement.GetString()!;
                if (!ids.Add(id) || !names.Add(name) || current && ++currentCount > 1)
                {
                    throw new InvalidOperationException(invalid);
                }

                layers[index++] = new ArcCadLayerInfo(
                    id,
                    name,
                    off,
                    frozen,
                    locked,
                    plot,
                    current);
            }

            if (currentCount != 1)
            {
                throw new InvalidOperationException(invalid);
            }

            return Array.AsReadOnly(layers);
        }
        catch (JsonException exception)
        {
            throw new InvalidOperationException(invalid, exception);
        }
    }

    private static bool TryGetBoolean(JsonElement element, string name, out bool value)
    {
        value = false;
        if (!element.TryGetProperty(name, out var property) ||
            property.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
        {
            return false;
        }

        value = property.GetBoolean();
        return true;
    }

    private static ArcCadPoint ParsePointArray(JsonElement element, string invalid)
    {
        if (element.ValueKind != JsonValueKind.Array || element.GetArrayLength() != 2)
        {
            throw new InvalidOperationException(invalid);
        }

        var values = element.EnumerateArray();
        if (!values.MoveNext() || !values.Current.TryGetDouble(out var x) ||
            !values.MoveNext() || !values.Current.TryGetDouble(out var y) ||
            !double.IsFinite(x) || !double.IsFinite(y))
        {
            throw new InvalidOperationException(invalid);
        }

        return new ArcCadPoint(x, y);
    }

    private static ArcCadRenderBatch ParseRenderBatch(
        JsonElement element,
        int vertexCount,
        string invalid)
    {
        if (element.ValueKind != JsonValueKind.Object ||
            !element.TryGetProperty("layer", out var layerElement) ||
            !layerElement.TryGetUInt64(out var layerId) ||
            !element.TryGetProperty("color", out var colorElement) ||
            !element.TryGetProperty("linetype", out var linetypeElement) ||
            !linetypeElement.TryGetUInt64(out var linetypeId) ||
            !element.TryGetProperty("strips", out var stripsElement) ||
            stripsElement.ValueKind != JsonValueKind.Array ||
            !element.TryGetProperty("markers", out var markersElement) ||
            markersElement.ValueKind != JsonValueKind.Array)
        {
            throw new InvalidOperationException(invalid);
        }

        var strips = new ArcCadRenderStrip[stripsElement.GetArrayLength()];
        var index = 0;
        foreach (var strip in stripsElement.EnumerateArray())
        {
            if (strip.ValueKind != JsonValueKind.Object ||
                !strip.TryGetProperty("entity", out var entityElement) ||
                !entityElement.TryGetUInt64(out var entityId) ||
                entityId == 0 ||
                !strip.TryGetProperty("offset", out var offsetElement) ||
                !offsetElement.TryGetUInt32(out var offset) ||
                !strip.TryGetProperty("count", out var countElement) ||
                !countElement.TryGetUInt32(out var count) ||
                count == 0 ||
                !strip.TryGetProperty("width", out var widthElement) ||
                !widthElement.TryGetSingle(out var width) ||
                !float.IsFinite(width) ||
                width < 0.0f ||
                !strip.TryGetProperty("polyWidth", out var polyWidthElement) ||
                !polyWidthElement.TryGetSingle(out var polyWidth) ||
                !float.IsFinite(polyWidth) ||
                polyWidth < 0.0f ||
                ((ulong)offset + count) * 2UL > (ulong)vertexCount)
            {
                throw new InvalidOperationException(invalid);
            }

            if (!strip.TryGetProperty("analyticLength", out var analyticLengthElement))
            {
                throw new InvalidOperationException(invalid);
            }

            double? analyticLength = null;
            if (analyticLengthElement.ValueKind != JsonValueKind.Null)
            {
                if (!analyticLengthElement.TryGetDouble(out var value) ||
                    !double.IsFinite(value) ||
                    value < 0.0)
                {
                    throw new InvalidOperationException(invalid);
                }

                analyticLength = value;
            }

            strips[index++] = new ArcCadRenderStrip(
                entityId,
                offset,
                count,
                width,
                polyWidth,
                analyticLength);
        }

        var markers = new ArcCadRenderMarker[markersElement.GetArrayLength()];
        index = 0;
        foreach (var marker in markersElement.EnumerateArray())
        {
            if (marker.ValueKind != JsonValueKind.Object ||
                !marker.TryGetProperty("entity", out var entityElement) ||
                !entityElement.TryGetUInt64(out var entityId) ||
                entityId == 0 ||
                !marker.TryGetProperty("x", out var xElement) ||
                !xElement.TryGetSingle(out var x) ||
                !marker.TryGetProperty("y", out var yElement) ||
                !yElement.TryGetSingle(out var y) ||
                !float.IsFinite(x) || !float.IsFinite(y))
            {
                throw new InvalidOperationException(invalid);
            }

            markers[index++] = new ArcCadRenderMarker(entityId, x, y);
        }

        return new ArcCadRenderBatch(
            layerId,
            ParseColor(colorElement, invalid),
            linetypeId,
            strips.AsMemory(),
            markers.AsMemory());
    }

    private static ArcCadRenderBatchKey ParseRenderBatchKey(
        JsonElement element,
        string invalid)
    {
        if (element.ValueKind != JsonValueKind.Object ||
            !element.TryGetProperty("layer", out var layerElement) ||
            !layerElement.TryGetUInt64(out var layerId) ||
            !element.TryGetProperty("color", out var colorElement) ||
            !element.TryGetProperty("linetype", out var linetypeElement) ||
            !linetypeElement.TryGetUInt64(out var linetypeId))
        {
            throw new InvalidOperationException(invalid);
        }

        return new ArcCadRenderBatchKey(
            layerId,
            ParseColor(colorElement, invalid),
            linetypeId);
    }

    private static ArcCadRgba ParseColor(JsonElement element, string invalid)
    {
        if (element.ValueKind != JsonValueKind.Array || element.GetArrayLength() != 4)
        {
            throw new InvalidOperationException(invalid);
        }

        Span<byte> color = stackalloc byte[4];
        var index = 0;
        foreach (var component in element.EnumerateArray())
        {
            if (!component.TryGetByte(out color[index++]))
            {
                throw new InvalidOperationException(invalid);
            }
        }

        return new ArcCadRgba(color[0], color[1], color[2], color[3]);
    }

    private static ArcCadLineResult ParseLineSuccess(JsonElement ok)
    {
        if (ok.ValueKind != JsonValueKind.Object ||
            !ok.TryGetProperty("txSeq", out var transactionSequenceElement) ||
            transactionSequenceElement.ValueKind != JsonValueKind.Number ||
            !transactionSequenceElement.TryGetUInt64(out var transactionSequence) ||
            !ok.TryGetProperty("created", out var created) ||
            created.ValueKind != JsonValueKind.Array ||
            created.GetArrayLength() != 1)
        {
            throw InvalidLineEnvelope();
        }

        var entities = created.EnumerateArray();
        if (!entities.MoveNext() ||
            entities.Current.ValueKind != JsonValueKind.Number ||
            !entities.Current.TryGetUInt64(out var entityId))
        {
            throw InvalidLineEnvelope();
        }

        string? message = null;
        if (ok.TryGetProperty("message", out var messageElement))
        {
            if (messageElement.ValueKind == JsonValueKind.String)
            {
                message = messageElement.GetString();
            }
            else if (messageElement.ValueKind != JsonValueKind.Null)
            {
                throw InvalidLineEnvelope();
            }
        }

        return new ArcCadLineResult(transactionSequence, entityId, message);
    }

    private static ArcCadMutationResult ParseMutationSuccess(JsonElement ok, string invalid)
    {
        if (ok.ValueKind != JsonValueKind.Object ||
            !ok.TryGetProperty("txSeq", out var transactionSequenceElement) ||
            transactionSequenceElement.ValueKind != JsonValueKind.Number ||
            !transactionSequenceElement.TryGetUInt64(out var transactionSequence) ||
            !ok.TryGetProperty("created", out var createdElement) ||
            createdElement.ValueKind != JsonValueKind.Array)
        {
            throw new InvalidOperationException(invalid);
        }

        var properties = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in ok.EnumerateObject())
        {
            if (!properties.Add(property.Name) ||
                property.Name is not ("txSeq" or "created" or "message"))
            {
                throw new InvalidOperationException(invalid);
            }
        }

        if (properties.Count is not (2 or 3))
        {
            throw new InvalidOperationException(invalid);
        }

        var created = new ulong[createdElement.GetArrayLength()];
        var seen = new HashSet<ulong>();
        var index = 0;
        foreach (var entityElement in createdElement.EnumerateArray())
        {
            if (!entityElement.TryGetUInt64(out var entityId) || entityId == 0 || !seen.Add(entityId))
            {
                throw new InvalidOperationException(invalid);
            }

            created[index++] = entityId;
        }

        string? message = null;
        if (ok.TryGetProperty("message", out var messageElement))
        {
            if (messageElement.ValueKind == JsonValueKind.String)
            {
                message = messageElement.GetString();
            }
            else if (messageElement.ValueKind != JsonValueKind.Null)
            {
                throw new InvalidOperationException(invalid);
            }
        }

        return new ArcCadMutationResult(transactionSequence, created.AsMemory(), message);
    }

    private static ArcCadHistoryResult ParseHistorySuccess(JsonElement ok, string invalid)
    {
        if (ok.ValueKind != JsonValueKind.Object ||
            !ok.TryGetProperty("txSeq", out var transactionSequence) ||
            transactionSequence.ValueKind != JsonValueKind.Null ||
            !ok.TryGetProperty("created", out var created) ||
            created.ValueKind != JsonValueKind.Array ||
            created.GetArrayLength() != 0)
        {
            throw new InvalidOperationException(invalid);
        }

        var properties = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in ok.EnumerateObject())
        {
            if (!properties.Add(property.Name) ||
                property.Name is not ("txSeq" or "created" or "message"))
            {
                throw new InvalidOperationException(invalid);
            }
        }

        if (properties.Count is not (2 or 3))
        {
            throw new InvalidOperationException(invalid);
        }

        string? message = null;
        if (ok.TryGetProperty("message", out var messageElement))
        {
            if (messageElement.ValueKind == JsonValueKind.String)
            {
                message = messageElement.GetString();
            }
            else if (messageElement.ValueKind != JsonValueKind.Null)
            {
                throw new InvalidOperationException(invalid);
            }
        }

        return new ArcCadHistoryResult(message);
    }

    private static ArcCadCommandException ParseCommandError(
        JsonElement error,
        string invalidEnvelope)
    {
        if (error.ValueKind != JsonValueKind.Object ||
            !error.TryGetProperty("code", out var codeElement) ||
            codeElement.ValueKind != JsonValueKind.String ||
            !error.TryGetProperty("message", out var messageElement) ||
            messageElement.ValueKind != JsonValueKind.String)
        {
            throw new InvalidOperationException(invalidEnvelope);
        }

        var code = codeElement.GetString();
        var message = messageElement.GetString();
        if (string.IsNullOrEmpty(code) || string.IsNullOrEmpty(message))
        {
            throw new InvalidOperationException(invalidEnvelope);
        }

        var detailJson = error.TryGetProperty("detail", out var detail)
            ? detail.GetRawText()
            : null;
        return new ArcCadCommandException(code, message, detailJson);
    }

    private static ArcCadCommandException ParseOpenArcfError(
        JsonElement error,
        string invalidEnvelope)
    {
        if (error.ValueKind != JsonValueKind.Object)
        {
            throw new InvalidOperationException(invalidEnvelope);
        }

        var properties = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in error.EnumerateObject())
        {
            if (!properties.Add(property.Name) ||
                property.Name is not ("code" or "message" or "detail"))
            {
                throw new InvalidOperationException(invalidEnvelope);
            }
        }

        if (!properties.Contains("code") ||
            !properties.Contains("message") ||
            properties.Count is not (2 or 3))
        {
            throw new InvalidOperationException(invalidEnvelope);
        }

        return ParseCommandError(error, invalidEnvelope);
    }

    private static InvalidOperationException InvalidLineEnvelope() =>
        new("af_session_execute_json returned an invalid LINE envelope.");

    internal unsafe string CopyAndFreeUtf8Result(
        string operation,
        AfStatus status,
        ref AfUtf8BufferNative result)
    {
        Exception? primaryError = null;
        try
        {
            ThrowIfSessionCallFailed(operation, status);
            return CopyUtf8Result(operation, result);
        }
        catch (Exception exception)
        {
            primaryError = exception;
            throw;
        }
        finally
        {
            try
            {
                ThrowIfFailed(
                    nameof(AfNative.Utf8BufferFree),
                    AfNative.Utf8BufferFree(ref result));
            }
            catch when (primaryError is not null)
            {
            }
        }
    }

    internal unsafe float[] CopyAndFreeF32Result(
        string operation,
        AfStatus status,
        ref AfF32BufferNative result)
    {
        Exception? primaryError = null;
        try
        {
            ThrowIfSessionCallFailed(operation, status);
            return CopyF32Result(operation, result);
        }
        catch (Exception exception)
        {
            primaryError = exception;
            throw;
        }
        finally
        {
            try
            {
                ThrowIfFailed(
                    nameof(AfNative.F32BufferFree),
                    AfNative.F32BufferFree(ref result));
            }
            catch when (primaryError is not null)
            {
            }
        }
    }

    internal unsafe byte[] CopyAndFreeByteResult(
        string operation,
        AfStatus status,
        ref AfByteBufferNative result)
    {
        Exception? primaryError = null;
        try
        {
            ThrowIfSessionCallFailed(operation, status);
            return CopyByteResult(operation, result);
        }
        catch (Exception exception)
        {
            primaryError = exception;
            throw;
        }
        finally
        {
            try
            {
                ThrowIfFailed(
                    nameof(AfNative.ByteBufferFree),
                    AfNative.ByteBufferFree(ref result));
            }
            catch when (primaryError is not null)
            {
            }
        }
    }

    internal unsafe ArcCadRenderDelta CopyParseAndFreeRenderDelta(
        AfStatus status,
        ref AfUtf8BufferNative control,
        ref AfF32BufferNative vertices)
    {
        ArcCadRenderDelta? delta = null;
        Exception? primaryError = null;
        try
        {
            ThrowIfSessionCallFailed(nameof(AfNative.SessionRenderDelta), status);
            delta = ParseRenderDelta(
                CopyUtf8Result(nameof(AfNative.SessionRenderDelta), control),
                CopyF32Result(nameof(AfNative.SessionRenderDelta), vertices));
        }
        catch (Exception exception)
        {
            primaryError = exception;
        }

        Exception? firstCleanupError = null;
        try
        {
            ThrowIfFailed(
                nameof(AfNative.Utf8BufferFree),
                AfNative.Utf8BufferFree(ref control));
        }
        catch (Exception exception)
        {
            firstCleanupError = exception;
        }

        try
        {
            ThrowIfFailed(
                nameof(AfNative.F32BufferFree),
                AfNative.F32BufferFree(ref vertices));
        }
        catch (Exception exception)
        {
            firstCleanupError ??= exception;
        }

        if (primaryError is not null)
        {
            ExceptionDispatchInfo.Capture(primaryError).Throw();
        }

        if (firstCleanupError is not null)
        {
            ExceptionDispatchInfo.Capture(firstCleanupError).Throw();
        }

        return delta!;
    }

    private static unsafe string CopyUtf8Result(
        string operation,
        AfUtf8BufferNative result)
    {
        if (result.Data == nint.Zero ||
            result.Length == 0 ||
            result.Owner == 0 ||
            result.Capacity < result.Length ||
            result.Length > (nuint)int.MaxValue)
        {
            throw new InvalidOperationException(
                $"{operation} returned invalid output metadata.");
        }

        return StrictUtf8.GetString(
            new ReadOnlySpan<byte>((void*)result.Data, (int)result.Length));
    }

    private static unsafe float[] CopyF32Result(
        string operation,
        AfF32BufferNative result)
    {
        if (result.Owner == 0)
        {
            if (result.Data != nint.Zero || result.Length != 0 || result.Capacity != 0)
            {
                throw new InvalidOperationException(
                    $"{operation} returned invalid empty output metadata.");
            }

            return Array.Empty<float>();
        }

        if (result.Data == nint.Zero ||
            result.Length == 0 ||
            result.Capacity < result.Length ||
            result.Length > (nuint)int.MaxValue)
        {
            throw new InvalidOperationException(
                $"{operation} returned invalid output metadata.");
        }

        return new ReadOnlySpan<float>(
            (void*)result.Data,
            (int)result.Length).ToArray();
    }

    internal static unsafe byte[] CopyByteResult(
        string operation,
        AfByteBufferNative result)
    {
        if (result.Data == nint.Zero ||
            result.Length == 0 ||
            result.Owner == 0 ||
            result.Capacity < result.Length ||
            result.Length > (nuint)int.MaxValue)
        {
            throw new InvalidOperationException(
                $"{operation} returned invalid output metadata.");
        }

        return new ReadOnlySpan<byte>(
            (void*)result.Data,
            (int)result.Length).ToArray();
    }

    private static void ThrowIfPointIsNotFinite(ArcCadPoint point, string parameterName)
    {
        if (!double.IsFinite(point.X) || !double.IsFinite(point.Y))
        {
            throw new ArgumentOutOfRangeException(
                parameterName,
                point,
                "Coordinates must be finite.");
        }
    }

    private static void ValidateOptionalPositive(double? value, string parameterName)
    {
        if (value is { } number && (!double.IsFinite(number) || number <= 0))
        {
            throw new ArgumentOutOfRangeException(
                parameterName,
                number,
                "Value must be finite and positive.");
        }
    }

    private static void ValidatePath(
        IReadOnlyList<ArcCadPoint> points,
        int minimumPointCount,
        string command)
    {
        ArgumentNullException.ThrowIfNull(points);
        if (points.Count < minimumPointCount)
        {
            throw new ArgumentException(
                $"{command} needs at least {minimumPointCount} points.",
                nameof(points));
        }

        for (var index = 0; index < points.Count; index++)
        {
            ThrowIfPointIsNotFinite(points[index], nameof(points));
            if (index > 0 &&
                Math.Abs(points[index].X - points[index - 1].X) +
                Math.Abs(points[index].Y - points[index - 1].Y) <= 0.000001)
            {
                throw new ArgumentException(
                    $"{command} needs distinct consecutive points.",
                    nameof(points));
            }
        }
    }

    private static string NormalizeRevisionCloudStyle(string style)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(style);
        var normalized = style.Trim().ToUpperInvariant();
        if (normalized is not ("NORMAL" or "CALLIGRAPHY"))
        {
            throw new ArgumentException(
                "REVCLOUD style must be NORMAL or CALLIGRAPHY.",
                nameof(style));
        }

        return normalized;
    }

    private ArcCadMutationResult CreateAxisXline(ArcCadPoint point, string mode)
    {
        ThrowIfPointIsNotFinite(point, nameof(point));
        return ExecuteMutation(
            "XLINE",
            JsonSerializer.Serialize(new
            {
                mode,
                p1 = new[] { point.X, point.Y },
            }));
    }

    private void ThrowIfUnavailable()
    {
        if (IsDisposed)
        {
            throw new ObjectDisposedException(nameof(ArcCadSession));
        }

        ThrowIfWrongThread();
    }

    internal void ThrowIfSessionCallFailed(string operation, AfStatus status)
    {
        if (status != AfStatus.Panic)
        {
            ThrowIfFailed(operation, status);
            return;
        }

        var callError = new InvalidOperationException(
            $"{operation} failed with native status {(uint)status}; the session was discarded.");
        try
        {
            _handle.CloseChecked();
        }
        catch (Exception discardError)
        {
            throw new AggregateException(callError, discardError);
        }

        throw callError;
    }

    private void ThrowIfWrongThread()
    {
        if (Environment.CurrentManagedThreadId != _ownerThreadId)
        {
            throw new InvalidOperationException(
                "ArcCadSession can only be used on its owner thread.");
        }
    }
}
