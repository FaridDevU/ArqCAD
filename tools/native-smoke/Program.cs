using System.Diagnostics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using ArcForge.Native;

if (args is ["--expect-missing-app-local"])
{
    var appLocalDllPath = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "af_ffi.dll"));
    Require(!File.Exists(appLocalDllPath), "Missing-DLL probe unexpectedly contains app-local af_ffi.dll.");
    var missingError = ExpectThrows<DllNotFoundException>(
        () => AfNative.AbiVersion(out _),
        "Missing app-local af_ffi.dll must fail during the first ABI query.");
    Require(
        missingError.Message.Contains(appLocalDllPath, StringComparison.OrdinalIgnoreCase),
        "Missing-DLL error must identify the required absolute app-local path.");
    Console.WriteLine("PASS MISSING_APP_LOCAL_DLL");
    return;
}

if (args.Length != 3)
{
    throw new ArgumentException(
        "Usage: ArcForge.Native.Smoke <af_ffi.dll path> <af_ffi.dll sha256> <libunwind.dll path>");
}

var expectedDllPath = Path.GetFullPath(args[0]);
var expectedDllHash = args[1].ToLowerInvariant();
var expectedUnwindPath = Path.GetFullPath(args[2]);

Require(File.Exists(expectedDllPath), $"Native DLL not found: {expectedDllPath}");
Require(File.Exists(expectedUnwindPath), $"Rust runtime not found: {expectedUnwindPath}");
var stagedDllHash = HashFile(expectedDllPath);
Require(
    string.Equals(stagedDllHash, expectedDllHash, StringComparison.OrdinalIgnoreCase),
    $"Staged DLL hash mismatch before native load: {stagedDllHash}");
RunMissingDllProbe(expectedDllPath, expectedUnwindPath);
Require(Marshal.SizeOf<AfVersionNative>() == 6, "AfVersion layout must be exactly 6 bytes.");
Require(
    Marshal.SizeOf<AfUtf8BufferNative>() == 4 * IntPtr.Size,
    "AfUtf8Buffer layout must be exactly four pointer-sized words.");
Require(
    Marshal.OffsetOf<AfUtf8BufferNative>(nameof(AfUtf8BufferNative.Data)).ToInt64() == 0,
    "AfUtf8Buffer.Data offset mismatch.");
Require(
    Marshal.OffsetOf<AfUtf8BufferNative>(nameof(AfUtf8BufferNative.Length)).ToInt64() ==
    IntPtr.Size,
    "AfUtf8Buffer.Length offset mismatch.");
Require(
    Marshal.OffsetOf<AfUtf8BufferNative>(nameof(AfUtf8BufferNative.Capacity)).ToInt64() ==
    2 * IntPtr.Size,
    "AfUtf8Buffer.Capacity offset mismatch.");
Require(
    Marshal.OffsetOf<AfUtf8BufferNative>(nameof(AfUtf8BufferNative.Owner)).ToInt64() ==
    3 * IntPtr.Size,
    "AfUtf8Buffer.Owner offset mismatch.");
Require(
    Marshal.SizeOf<AfF32BufferNative>() == 4 * IntPtr.Size,
    "AfF32Buffer layout must be exactly four pointer-sized words.");
Require(
    Marshal.OffsetOf<AfF32BufferNative>(nameof(AfF32BufferNative.Data)).ToInt64() == 0,
    "AfF32Buffer.Data offset mismatch.");
Require(
    Marshal.OffsetOf<AfF32BufferNative>(nameof(AfF32BufferNative.Length)).ToInt64() ==
    IntPtr.Size,
    "AfF32Buffer.Length offset mismatch.");
Require(
    Marshal.OffsetOf<AfF32BufferNative>(nameof(AfF32BufferNative.Capacity)).ToInt64() ==
    2 * IntPtr.Size,
    "AfF32Buffer.Capacity offset mismatch.");
Require(
    Marshal.OffsetOf<AfF32BufferNative>(nameof(AfF32BufferNative.Owner)).ToInt64() ==
    3 * IntPtr.Size,
    "AfF32Buffer.Owner offset mismatch.");
Require(
    Marshal.SizeOf<AfByteBufferNative>() == 4 * IntPtr.Size,
    "AfByteBuffer layout must be exactly four pointer-sized words.");
Require(
    Marshal.OffsetOf<AfByteBufferNative>(nameof(AfByteBufferNative.Data)).ToInt64() == 0,
    "AfByteBuffer.Data offset mismatch.");
Require(
    Marshal.OffsetOf<AfByteBufferNative>(nameof(AfByteBufferNative.Length)).ToInt64() ==
    IntPtr.Size,
    "AfByteBuffer.Length offset mismatch.");
Require(
    Marshal.OffsetOf<AfByteBufferNative>(nameof(AfByteBufferNative.Capacity)).ToInt64() ==
    2 * IntPtr.Size,
    "AfByteBuffer.Capacity offset mismatch.");
Require(
    Marshal.OffsetOf<AfByteBufferNative>(nameof(AfByteBufferNative.Owner)).ToInt64() ==
    3 * IntPtr.Size,
    "AfByteBuffer.Owner offset mismatch.");

var versionStatus = AfNative.AbiVersion(out var nativeVersion);
Require(versionStatus == AfStatus.Ok, $"af_abi_version returned {(uint)versionStatus}.");
Require(
    nativeVersion is { Major: 0, Minor: 7, Patch: 0 },
    $"Expected ABI 0.7.0, got {nativeVersion.Major}.{nativeVersion.Minor}.{nativeVersion.Patch}.");

using var process = Process.GetCurrentProcess();
var loadedDllPath = FindModulePath(process, "af_ffi.dll");
var loadedUnwindPath = FindModulePath(process, "libunwind.dll");
Require(PathsEqual(loadedDllPath, expectedDllPath), $"Loaded unexpected af_ffi.dll: {loadedDllPath}");
Require(
    PathsEqual(loadedUnwindPath, expectedUnwindPath),
    $"Loaded unexpected libunwind.dll: {loadedUnwindPath}");
var loadedDllHash = HashFile(loadedDllPath);
Require(
    string.Equals(loadedDllHash, expectedDllHash, StringComparison.OrdinalIgnoreCase),
    $"Loaded DLL hash mismatch: {loadedDllHash}");

ExpectThrows<NotSupportedException>(
    () => ArcCadSession.ValidateAbi(new AfVersionNative(1, 7, 0)),
    "ABI major mismatch must be rejected.");
ExpectThrows<NotSupportedException>(
    () => ArcCadSession.ValidateAbi(new AfVersionNative(0, 6, 99)),
    "ABI 0.6 must be rejected before persistence symbols are used.");
Require(
    ArcCadSession.ValidateAbi(new AfVersionNative(0, 7, 0)) == new AfAbiVersion(0, 7, 0),
    "ABI 0.7 must be accepted.");
Require(
    ArcCadSession.ValidateAbi(new AfVersionNative(0, 8, 0)) == new AfAbiVersion(0, 8, 0),
    "ABI 0.8 must be accepted as a later additive minor.");

var statusError = ExpectThrows<InvalidOperationException>(
    () => ArcCadSession.ThrowIfFailed("synthetic_operation", AfStatus.Internal),
    "A failing native status must throw.");
Require(
    statusError.Message.Contains("synthetic_operation", StringComparison.Ordinal) &&
    statusError.Message.Contains("4", StringComparison.Ordinal),
    "Native status exception must include operation and numeric status.");

var parsedOpenWarnings = ArcCadSession.ParseOpenArcfResult(
    """{"ok":["normalized layer","recovered style"]}""");
Require(
    parsedOpenWarnings.SequenceEqual(new[] { "normalized layer", "recovered style" }) &&
    parsedOpenWarnings is ICollection<string> { IsReadOnly: true },
    "Open parser must preserve warnings in a read-only collection.");
var parsedOpenError = ExpectThrows<ArcCadCommandException>(
    () => ArcCadSession.ParseOpenArcfResult(
        """{"error":{"code":"invalid_document","message":"bad file","detail":{"offset":7}}}"""),
    "Structured open errors must remain typed.");
Require(
    parsedOpenError is { Code: "invalid_document", Message: "bad file" } &&
    parsedOpenError.DetailJson == """{"offset":7}""",
    "Open error code, message, and detail must be preserved.");
var malformedOpen = ExpectThrows<InvalidOperationException>(
    () => ArcCadSession.ParseOpenArcfResult("{"),
    "Malformed open JSON must fail explicitly.");
Require(
    malformedOpen.InnerException is JsonException,
    "Malformed open JSON must retain JsonException as its inner exception.");
ExpectThrows<InvalidOperationException>(
    () => ArcCadSession.ParseOpenArcfResult("""{"ok":[],"extra":true}"""),
    "Open success envelopes must reject extra root fields.");
ExpectThrows<InvalidOperationException>(
    () => ArcCadSession.ParseOpenArcfResult("""{"ok":[1]}"""),
    "Open warnings must be strings.");
ExpectThrows<InvalidOperationException>(
    () => ArcCadSession.ParseOpenArcfResult(
        """{"error":{"code":"x","message":"y","extra":true}}"""),
    "Open errors must reject unknown fields.");

ExpectThrows<InvalidOperationException>(
    () => ArcCadSession.CopyByteResult(
        "synthetic_save",
        new AfByteBufferNative { Length = 1, Capacity = 1, Owner = 1 }),
    "Byte copy must reject a null payload pointer.");
ExpectThrows<InvalidOperationException>(
    () => ArcCadSession.CopyByteResult(
        "synthetic_save",
        new AfByteBufferNative { Data = 1, Length = 1, Capacity = 1 }),
    "Byte copy must reject a zero owner.");
ExpectThrows<InvalidOperationException>(
    () => ArcCadSession.CopyByteResult(
        "synthetic_save",
        new AfByteBufferNative { Data = 1, Length = 2, Capacity = 1, Owner = 1 }),
    "Byte copy must reject capacity below length.");
ExpectThrows<InvalidOperationException>(
    () => ArcCadSession.CopyByteResult(
        "synthetic_save",
        new AfByteBufferNative { Data = 1, Capacity = 1, Owner = 1 }),
    "Byte copy must reject an empty save payload.");
if (IntPtr.Size > sizeof(int))
{
    var overManagedLimit = (nuint)int.MaxValue + 1;
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.CopyByteResult(
            "synthetic_save",
            new AfByteBufferNative
            {
                Data = 1,
                Length = overManagedLimit,
                Capacity = overManagedLimit,
                Owner = 1,
            }),
        "Byte copy must reject lengths above int.MaxValue.");
}

var session = ArcCadSession.Create();
var handle = session.DangerousHandle;
var copiedLineVertices = Array.Empty<float>();
try
{
    Require(handle != 0, "af_session_create returned a zero handle.");
    Require(session.AbiVersion == new AfAbiVersion(0, 7, 0), "Session ABI version mismatch.");
    Require(
        !session.SafeHandle.IsInvalid && !session.SafeHandle.IsClosed,
        "ArcCadSession must own a live SafeHandle.");

    var wrongNativeThreadSaveStatus =
        AfNative.SessionSaveArcf(session.SafeHandle, out var wrongNativeThreadBytes);
    Require(
        wrongNativeThreadSaveStatus == AfStatus.WrongThread,
        "A raw save outside the native worker must fail with WRONG_THREAD.");
    RequireByteEmpty(
        wrongNativeThreadBytes,
        "A wrong-thread raw save must return a canonical empty byte buffer.");

    var wrongNativeThreadOpenStatus = AfNative.SessionOpenArcfJson(
        session.SafeHandle,
        nint.Zero,
        0,
        out var wrongNativeThreadOpenResult);
    Require(
        wrongNativeThreadOpenStatus == AfStatus.WrongThread,
        "A raw open outside the native worker must fail with WRONG_THREAD.");
    RequireEmpty(
        wrongNativeThreadOpenResult,
        "A wrong-thread raw open must return a canonical empty UTF-8 buffer.");

    var emptySavedBytes = session.SaveArcf();
    Require(emptySavedBytes.Length > 0, "An empty document must still serialize to non-empty ARCF bytes.");

    var wrongNativeThreadStatus =
        AfNative.SessionRenderVertices(session.SafeHandle, out var wrongNativeThreadVertices);
    Require(
        wrongNativeThreadStatus == AfStatus.WrongThread,
        "A raw session call outside the native worker must fail with WRONG_THREAD.");
    RequireF32Empty(
        wrongNativeThreadVertices,
        "A wrong-thread raw call must return a canonical empty buffer.");

    var emptyManagedVertices = session.RenderVertices();
    Require(
        ReferenceEquals(emptyManagedVertices, Array.Empty<float>()),
        "An empty session must return Array.Empty<float>().");

    var (emptyVerticesStatus, emptyVertices) = RenderVerticesRaw(session);
    Require(
        emptyVerticesStatus == AfStatus.Ok,
        $"Empty raw render vertices returned {(uint)emptyVerticesStatus}.");
    RequireF32Empty(emptyVertices, "Empty raw render vertices must be canonical empty.");
    Require(
        AfNative.F32BufferFree(ref emptyVertices) == AfStatus.Ok,
        "Free of an empty f32 buffer must succeed.");
    Require(
        AfNative.F32BufferFree(ref emptyVertices) == AfStatus.Ok,
        "Free of an empty f32 buffer must be idempotent.");

    using (var emptyRender = JsonDocument.Parse(session.RenderFullJson()))
    {
        var root = emptyRender.RootElement;
        RequireExactProperties(root, "batches", "vertices", "ltscale");
        Require(root.GetProperty("batches").GetArrayLength() == 0, "Empty render must have no batches.");
        Require(root.GetProperty("vertices").GetArrayLength() == 0, "Empty render must have no vertices.");
        Require(root.GetProperty("ltscale").GetDouble() == 1.0, "Empty render LTSCALE must be 1.");
    }

    ExpectThrows<ArgumentNullException>(
        () => session.ExecuteJson(null!, "null"),
        "A null command must be rejected.");
    ExpectThrows<ArgumentNullException>(
        () => session.ExecuteJson("LINE", null!),
        "Null args JSON must be rejected.");

    Require(
        session.ParsePoint("12,34") == new ArcCadPoint(12, 34),
        "Absolute point parsing must preserve both coordinates.");
    Require(
        session.ParsePoint("@2,3", new ArcCadPoint(10, 20)) == new ArcCadPoint(12, 23),
        "Relative point parsing must resolve against the supplied base.");
    var relativeWithoutBase = ExpectThrows<ArcCadCommandException>(
        () => session.ParsePoint("@2,3"),
        "Relative input without a base must remain a typed domain error.");
    Require(
        relativeWithoutBase.Code == "parse_error",
        "Relative input without base must preserve parse_error.");
    ExpectThrows<ArgumentNullException>(
        () => session.ParsePoint(null!),
        "Null point input must be rejected.");
    ExpectThrows<EncoderFallbackException>(
        () => session.ParsePoint("\uD800"),
        "Invalid UTF-16 point input must fail before native execution.");
    Require(session.Snap(new ArcCadPoint(0, 0), 1).Count == 0, "Empty session must have no snaps.");
    ExpectThrows<ArgumentOutOfRangeException>(
        () => session.Snap(new ArcCadPoint(0, 0), 0),
        "Zero snap radius must be rejected before native execution.");
    Require(
        session.SelectAt(new ArcCadPoint(0, 0), 1).Count == 0,
        "Empty session selection must be empty.");
    ExpectThrows<ArgumentOutOfRangeException>(
        () => session.SelectAt(new ArcCadPoint(double.NaN, 0), 1),
        "Non-finite selection coordinates must be rejected before native execution.");
    ExpectThrows<ArgumentOutOfRangeException>(
        () => session.SelectAt(new ArcCadPoint(0, 0), 0),
        "Zero selection tolerance must be rejected before native execution.");

    var renderedLine = session.CreateLine(
        new ArcCadPoint(0, 0),
        new ArcCadPoint(10, 20));
    Require(
        renderedLine.TransactionSequence == 0,
        "Parse and snap queries must leave the first LINE at txSeq 0.");
    Require(renderedLine.EntityId > 0, "Rendered LINE must return an entity id.");

    var snaps = session.Snap(new ArcCadPoint(0.1, 0.1), 1.0);
    Require(snaps.Count > 0, "LINE endpoint snap must be reachable.");
    Require(
        snaps[0] is
        {
            Point: { X: 0, Y: 0 },
            Kind: "endpoint",
            EntityId: var snapEntity,
        } && snapEntity == renderedLine.EntityId && snaps[0].Distance >= 0,
        "The best snap must be the exact LINE endpoint with its entity id.");

    var lineDelta = session.RenderDelta();
    Require(lineDelta.Upserts.Length == 1, "First LINE delta must contain one upsert.");
    Require(lineDelta.Removes.Length == 0, "First LINE delta must contain no removes.");
    Require(lineDelta.LinetypeScale == 1.0, "LINE delta LTSCALE must be 1.");
    RequireFloatBits(
        lineDelta.Vertices.ToArray(),
        [0f, 0f, 10f, 20f],
        "Managed render delta must preserve LINE geometry bit-exactly.");
    var deltaBatch = lineDelta.Upserts.Span[0];
    Require(deltaBatch.Strips.Length == 1, "LINE delta must contain one strip.");
    Require(deltaBatch.Markers.Length == 0, "LINE delta must contain no markers.");
    var deltaStrip = deltaBatch.Strips.Span[0];
    var observedDeltaAnalyticLength = deltaStrip.AnalyticLength?.ToString("R") ?? "null";
    var expectedLineAnalyticLength = Math.Sqrt(500.0);
    Require(
        deltaStrip is { EntityId: var deltaEntity, Offset: 0, Count: 2 } &&
        deltaEntity == renderedLine.EntityId &&
        float.IsFinite(deltaStrip.Width) && deltaStrip.Width >= 0 &&
        deltaStrip.PolyWidth == 0 &&
        deltaStrip.AnalyticLength is { } deltaAnalyticLength &&
        Math.Abs(deltaAnalyticLength - expectedLineAnalyticLength) <= 1e-12,
        $"LINE delta strip must use polyWidth=0 and analyticLength=sqrt(500); observed " +
        $"polyWidth={deltaStrip.PolyWidth:R}, analyticLength={observedDeltaAnalyticLength}.");

    var emptyDelta = session.RenderDelta();
    Require(
        emptyDelta.Upserts.Length == 0 &&
        emptyDelta.Removes.Length == 0 &&
        emptyDelta.Vertices.Length == 0 &&
        emptyDelta.LinetypeScale == 1.0,
        "A second delta read must be empty and preserve LTSCALE.");

    copiedLineVertices = session.RenderVertices();
    RequireFloatBits(
        copiedLineVertices,
        [0f, 0f, 10f, 20f],
        "Managed render vertices must preserve the LINE coordinates bit-exactly.");

    AfF32BufferNative rawVertices = default;
    try
    {
        var rawCall = RenderVerticesRaw(session);
        var rawVerticesStatus = rawCall.Status;
        rawVertices = rawCall.Buffer;
        Require(
            rawVerticesStatus == AfStatus.Ok,
            $"Raw render vertices returned {(uint)rawVerticesStatus}.");
        RequireFloatBits(
            CopyF32(rawVertices),
            [0f, 0f, 10f, 20f],
            "Raw render vertices must preserve the LINE coordinates bit-exactly.");

        var staleVertices = rawVertices;
        var freeVerticesStatus = AfNative.F32BufferFree(ref rawVertices);
        Require(
            freeVerticesStatus == AfStatus.Ok,
            $"Raw f32 free returned {(uint)freeVerticesStatus}.");
        RequireF32Empty(rawVertices, "Successful f32 free must zero all buffer fields.");
        Require(
            AfNative.F32BufferFree(ref rawVertices) == AfStatus.Ok,
            "Free of canonical empty f32 buffer must be idempotent.");

        var staleVerticesBeforeFree = staleVertices;
        Require(
            AfNative.F32BufferFree(ref staleVertices) == AfStatus.InvalidHandle,
            "Free of stale f32 owner must return INVALID_HANDLE.");
        Require(
            F32BuffersEqual(staleVertices, staleVerticesBeforeFree),
            "Failed stale f32 free must leave all metadata unchanged.");
    }
    finally
    {
        ReleaseF32IfOwned(ref rawVertices);
    }

    RequireFloatBits(
        copiedLineVertices,
        [0f, 0f, 10f, 20f],
        "Managed vertices must survive native f32 free.");
    RequireSingleLineRender(session.RenderFullJson(), renderedLine.EntityId);

    var lineJson = session.ExecuteJson("LINE", """{"p1":[0,0],"p2":[1,1]}""");
    ulong lineTransactionSequence;
    using (var lineDocument = JsonDocument.Parse(lineJson))
    {
        var ok = lineDocument.RootElement.GetProperty("ok");
        Require(ok.GetProperty("txSeq").ValueKind == JsonValueKind.Number, "LINE txSeq must be numeric.");
        Require(
            ok.GetProperty("created").GetArrayLength() == 1,
            "LINE must return exactly one created entity id.");
        lineTransactionSequence = ok.GetProperty("txSeq").GetUInt64();
        var created = ok.GetProperty("created").EnumerateArray();
        Require(created.MoveNext(), "LINE created id is missing.");
        Require(created.Current.GetUInt64() > 0, "Raw LINE created id must be nonzero.");
    }
    Require(
        lineTransactionSequence == checked(renderedLine.TransactionSequence + 1),
        "RenderFullJson must not advance the transaction sequence.");

    var firstTypedLine = session.CreateLine(
        new ArcCadPoint(10, 20),
        new ArcCadPoint(30, 40));
    Require(
        firstTypedLine.TransactionSequence == checked(lineTransactionSequence + 1),
        "Raw and typed LINE transactions must remain consecutive.");
    Require(firstTypedLine.EntityId > 0, "Typed LINE must return an entity id.");

    ExpectThrows<ArgumentOutOfRangeException>(
        () => session.CreateLine(
            new ArcCadPoint(double.NaN, 0),
            new ArcCadPoint(1, 1)),
        "NaN coordinates must be rejected before native execution.");
    ExpectThrows<ArgumentOutOfRangeException>(
        () => session.CreateLine(
            new ArcCadPoint(0, 0),
            new ArcCadPoint(double.PositiveInfinity, 1)),
        "Infinite coordinates must be rejected before native execution.");

    var secondTypedLine = session.CreateLine(
        new ArcCadPoint(-5, 7.5),
        new ArcCadPoint(12.25, -8));
    Require(
        secondTypedLine.TransactionSequence == checked(firstTypedLine.TransactionSequence + 1),
        "Rejected non-finite inputs must not advance the transaction sequence.");
    Require(
        secondTypedLine.EntityId != firstTypedLine.EntityId,
        "Separate typed LINE commands must return distinct entity ids.");

    var degenerateLine = session.CreateLine(
        new ArcCadPoint(3, 3),
        new ArcCadPoint(3, 3));
    Require(
        degenerateLine.TransactionSequence == checked(secondTypedLine.TransactionSequence + 1),
        "Degenerate finite LINE must still create one transaction.");
    Require(
        !string.IsNullOrEmpty(degenerateLine.Message) &&
        degenerateLine.Message.Contains("zero-length", StringComparison.Ordinal),
        "Degenerate LINE warning must survive the typed gateway.");

    var zeroLine = ArcCadSession.ParseLineResult(
        """{"ok":{"txSeq":0,"created":[0]}}""");
    Require(
        zeroLine is { TransactionSequence: 0, EntityId: 0, Message: null },
        "Typed parser must preserve zero u64 values.");

    var maxLine = ArcCadSession.ParseLineResult(
        """{"ok":{"txSeq":18446744073709551615,"created":[18446744073709551615]}}""");
    Require(
        maxLine is
        {
            TransactionSequence: ulong.MaxValue,
            EntityId: ulong.MaxValue,
            Message: null,
        },
        "Typed parser must preserve ulong.MaxValue without floating-point conversion.");

    var commandError = ExpectThrows<ArcCadCommandException>(
        () => ArcCadSession.ParseLineResult(
            """{"error":{"code":"layer_locked","message":"layer is locked","detail":{"layer":18446744073709551615}}}"""),
        "Domain error envelope must become ArcCadCommandException.");
    Require(commandError.Code == "layer_locked", "Domain error code must be preserved.");
    Require(commandError.Message == "layer is locked", "Domain error message must be preserved.");
    Require(
        commandError.DetailJson == """{"layer":18446744073709551615}""",
        "Domain error detail JSON must be preserved.");

    var invalidJsonError = ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseLineResult("{"),
        "Invalid LINE JSON must become InvalidOperationException.");
    Require(
        invalidJsonError.InnerException is JsonException,
        "Invalid LINE JSON must retain JsonException as its inner exception.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseLineResult(
            """{"ok":{"txSeq":1,"created":[1]},"error":{"code":"x","message":"y"}}"""),
        "Envelope containing both ok and error must be rejected.");

    Require(
        ArcCadSession.ParsePointResult("""{"ok":{"point":[1.5,-2]}}""") ==
        new ArcCadPoint(1.5, -2),
        "Typed point parser must preserve finite doubles.");
    var pointError = ExpectThrows<ArcCadCommandException>(
        () => ArcCadSession.ParsePointResult(
            """{"error":{"code":"not_a_point","message":"not a point"}}"""),
        "Point error envelope must become ArcCadCommandException.");
    Require(
        pointError is { Code: "not_a_point", DetailJson: null },
        "Point error without detail must remain typed and omit detail.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParsePointResult(
            """{"ok":{"point":[1,2]},"error":{"code":"x","message":"y"}}"""),
        "Point envelope containing both branches must be rejected.");

    var parsedSnaps = ArcCadSession.ParseSnaps(
        """[{"point":[1,2],"kind":"endpoint","entity":7,"dist":0.25}]""");
    Require(
        parsedSnaps.Count == 1 &&
        parsedSnaps[0] == new ArcCadSnap(new ArcCadPoint(1, 2), "endpoint", 7, 0.25),
        "Typed snap parser must preserve every field.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseSnaps(
            """[{"point":[1,2],"kind":"endpoint","entity":0,"dist":0}]"""),
        "Snap parser must reject a zero entity id.");

    Require(ArcCadSession.ParseSelection("[]").Count == 0, "Empty selection JSON must parse.");
    var parsedSelection = ArcCadSession.ParseSelection(
        """[1,18446744073709551615]""");
    Require(
        parsedSelection.SequenceEqual(new[] { 1UL, ulong.MaxValue }),
        "Selection parser must preserve opaque u64 ids.");
    Require(
        parsedSelection is ICollection<ulong> { IsReadOnly: true },
        "Selection parser must return a read-only collection.");
    var malformedSelection = ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseSelection("["),
        "Malformed selection JSON must fail explicitly.");
    Require(
        malformedSelection.InnerException is JsonException,
        "Malformed selection JSON must retain JsonException as its inner exception.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseSelection("[0]"),
        "Selection parser must reject a zero entity id.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseSelection("[7,7]"),
        "Selection parser must reject duplicate entity ids.");

    Require(
        ArcCadSession.ParseHistoryResult(
            """{"ok":{"txSeq":null,"created":[]}}""").Message is null,
        "History parser must accept an absent message.");
    Require(
        ArcCadSession.ParseHistoryResult(
            """{"ok":{"txSeq":null,"created":[],"message":null}}""").Message is null,
        "History parser must accept a null message.");
    Require(
        ArcCadSession.ParseHistoryResult(
            """{"ok":{"txSeq":null,"created":[],"message":"restored"}}""").Message ==
        "restored",
        "History parser must preserve a string message.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseHistoryResult(
            """{"ok":{"txSeq":1,"created":[]}}"""),
        "History parser must reject a numeric transaction sequence.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseHistoryResult(
            """{"ok":{"txSeq":null,"created":[1]}}"""),
        "History parser must reject a non-empty created list.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseHistoryResult(
            """{"ok":{"txSeq":null,"created":[],"message":1}}"""),
        "History parser must reject a non-string message.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseHistoryResult(
            """{"ok":{"txSeq":null,"created":[],"extra":true}}"""),
        "History parser must reject unknown success fields.");
    var historyCommandError = ExpectThrows<ArcCadCommandException>(
        () => ArcCadSession.ParseHistoryResult(
            """{"error":{"code":"nothing_to_undo","message":"nothing to undo"}}"""),
        "History domain errors must remain typed.");
    Require(
        historyCommandError is { Code: "nothing_to_undo", Message: "nothing to undo" },
        "History domain error fields must be preserved.");

    using (var emptyHistorySession = ArcCadSession.Create())
    {
        Require(
            !string.IsNullOrEmpty(ExpectThrows<ArcCadCommandException>(
                () => emptyHistorySession.Undo(),
                "UNDO without history must propagate its command error.").Code),
            "UNDO command errors must preserve their code.");
        Require(
            !string.IsNullOrEmpty(ExpectThrows<ArcCadCommandException>(
                () => emptyHistorySession.Redo(),
                "REDO without history must propagate its command error.").Code),
            "REDO command errors must preserve their code.");
    }

    using (var historySession = ArcCadSession.Create())
    {
        var firstHistoryLine = historySession.CreateLine(
            new ArcCadPoint(0, 0),
            new ArcCadPoint(10, 0));
        Require(firstHistoryLine.TransactionSequence == 0, "First history LINE must use txSeq 0.");
        RequireDeltaEntities(
            historySession.RenderDelta(),
            [firstHistoryLine.EntityId],
            "First history LINE delta");

        var selected = historySession.SelectAt(new ArcCadPoint(5, 0.25), 1);
        Require(
            selected.Count == 1 && selected[0] == firstHistoryLine.EntityId,
            "SelectAt must return exactly the hit LINE id.");
        Require(
            historySession.SelectAt(new ArcCadPoint(100, 100), 1).Count == 0,
            "SelectAt miss must return an empty effective selection.");

        var secondHistoryLine = historySession.CreateLine(
            new ArcCadPoint(0, 10),
            new ArcCadPoint(10, 10));
        Require(
            secondHistoryLine.TransactionSequence == 1,
            "Selection queries must leave the second LINE at txSeq 1.");
        RequireDeltaEntities(
            historySession.RenderDelta(),
            [firstHistoryLine.EntityId, secondHistoryLine.EntityId],
            "Second history LINE delta");

        historySession.Undo();
        RequireDeltaEntities(
            historySession.RenderDelta(),
            [firstHistoryLine.EntityId],
            "UNDO delta");

        historySession.Redo();
        RequireDeltaEntities(
            historySession.RenderDelta(),
            [firstHistoryLine.EntityId, secondHistoryLine.EntityId],
            "REDO delta");

        var lineAfterHistory = historySession.CreateLine(
            new ArcCadPoint(0, 20),
            new ArcCadPoint(10, 20));
        Require(
            lineAfterHistory.TransactionSequence == 2,
            "UNDO/REDO must not consume the next LINE transaction sequence.");
    }

    var parsedDelta = ArcCadSession.ParseRenderDelta(
        """{"upserts":[{"layer":1,"color":[255,255,255,255],"linetype":2,"strips":[{"entity":3,"offset":0,"count":2,"width":0.25,"polyWidth":0,"analyticLength":null}],"markers":[]}],"removes":[],"vertices":[],"ltscale":1}""",
        [0f, 0f, 10f, 20f]);
    Require(parsedDelta.Upserts.Length == 1, "Typed delta parser must preserve one batch.");
    var parsedStrip = parsedDelta.Upserts.Span[0].Strips.Span[0];
    Require(
        parsedStrip is
        {
            EntityId: 3,
            Offset: 0,
            Count: 2,
            Width: 0.25f,
            PolyWidth: 0,
            AnalyticLength: null,
        },
        "Typed delta parser must preserve batch and strip control data.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseRenderDelta(
            """{"upserts":[],"removes":[],"vertices":[0],"ltscale":1}""",
            []),
        "Delta parser must reject geometry duplicated in control JSON.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseRenderDelta(
            """{"upserts":[],"removes":[],"vertices":[],"ltscale":1}""",
            [0f]),
        "Delta parser must reject an odd companion float count.");
    var malformedDelta = ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseRenderDelta("{", []),
        "Malformed delta JSON must fail explicitly.");
    Require(
        malformedDelta.InnerException is JsonException,
        "Malformed delta JSON must retain JsonException as its inner exception.");
    ExpectThrows<InvalidOperationException>(
        () => ArcCadSession.ParseRenderDelta(
            """{"upserts":[{"layer":1,"color":[255,255,255,255],"linetype":2,"strips":[{"entity":3,"offset":1,"count":2,"width":0.25,"polyWidth":0,"analyticLength":null}],"markers":[]}],"removes":[],"vertices":[],"ltscale":1}""",
            [0f, 0f, 10f, 20f]),
        "Delta parser must reject a strip outside its companion geometry.");

    using (var cleanupSession = ArcCadSession.Create())
    {
        cleanupSession.CreateLine(new ArcCadPoint(0, 0), new ArcCadPoint(10, 20));
        var cleanupCall = RenderDeltaRaw(cleanupSession);
        Require(cleanupCall.Status == AfStatus.Ok, "Cleanup seam requires an owned delta.");
        var originalControl = cleanupCall.Control;
        var tamperedControl = cleanupCall.Control;
        var cleanupVertices = cleanupCall.Vertices;
        try
        {
            tamperedControl.Capacity++;
            Exception? cleanupFailure = null;
            try
            {
                cleanupSession.CopyParseAndFreeRenderDelta(
                    AfStatus.Ok,
                    ref tamperedControl,
                    ref cleanupVertices);
            }
            catch (Exception exception)
            {
                cleanupFailure = exception;
            }

            Require(
                cleanupFailure is InvalidOperationException &&
                cleanupFailure.Message.Contains(
                    nameof(AfNative.Utf8BufferFree),
                    StringComparison.Ordinal),
                "Without a body failure, the first cleanup failure must be reported.");
            RequireF32Empty(
                cleanupVertices,
                "The f32 owner must be freed even when the UTF-8 free fails first.");
            var staleVertices = cleanupCall.Vertices;
            Require(
                AfNative.F32BufferFree(ref staleVertices) == AfStatus.InvalidHandle,
                "The second owner must actually be released after the first free fails.");
        }
        finally
        {
            if (tamperedControl.Owner != 0)
            {
                tamperedControl = originalControl;
                ReleaseIfOwned(ref tamperedControl);
            }

            ReleaseF32IfOwned(ref cleanupVertices);
        }

        cleanupSession.CreateLine(new ArcCadPoint(1, 1), new ArcCadPoint(2, 2));
        var primaryCall = RenderDeltaRaw(cleanupSession);
        Require(primaryCall.Status == AfStatus.Ok, "Primary-error seam requires an owned delta.");
        originalControl = primaryCall.Control;
        tamperedControl = primaryCall.Control;
        cleanupVertices = primaryCall.Vertices;
        try
        {
            tamperedControl.Length = 0;
            Exception? primaryFailure = null;
            try
            {
                cleanupSession.CopyParseAndFreeRenderDelta(
                    AfStatus.Ok,
                    ref tamperedControl,
                    ref cleanupVertices);
            }
            catch (Exception exception)
            {
                primaryFailure = exception;
            }

            Require(
                primaryFailure is InvalidOperationException &&
                primaryFailure.Message.Contains(
                    nameof(AfNative.SessionRenderDelta),
                    StringComparison.Ordinal) &&
                !primaryFailure.Message.Contains(
                    nameof(AfNative.Utf8BufferFree),
                    StringComparison.Ordinal),
                "A primary copy/parse failure must take precedence over cleanup failures.");
            RequireF32Empty(
                cleanupVertices,
                "Primary failure must not prevent release of the f32 owner.");
            var staleVertices = primaryCall.Vertices;
            Require(
                AfNative.F32BufferFree(ref staleVertices) == AfStatus.InvalidHandle,
                "Primary failure must still release the second owner.");
        }
        finally
        {
            if (tamperedControl.Owner != 0)
            {
                tamperedControl = originalControl;
                ReleaseIfOwned(ref tamperedControl);
            }

            ReleaseF32IfOwned(ref cleanupVertices);
        }

        var utf8Call = RenderFullRaw(cleanupSession);
        Require(
            utf8Call.Status == AfStatus.Ok && utf8Call.Buffer.Owner != 0,
            "UTF-8 status-failure seam requires a real owner.");
        var failedUtf8 = utf8Call.Buffer;
        var staleUtf8 = failedUtf8;
        try
        {
            var utf8StatusError = ExpectThrows<InvalidOperationException>(
                () => cleanupSession.CopyAndFreeUtf8Result(
                    "synthetic_utf8_status",
                    AfStatus.Internal,
                    ref failedUtf8),
                "Failed UTF-8 status must propagate after cleanup.");
            Require(
                utf8StatusError.Message.Contains("synthetic_utf8_status", StringComparison.Ordinal) &&
                utf8StatusError.Message.Contains("4", StringComparison.Ordinal),
                "UTF-8 status error must remain primary.");
            RequireEmpty(failedUtf8, "Failed UTF-8 status must release and zero its owner.");
            var staleUtf8BeforeFree = staleUtf8;
            Require(
                AfNative.Utf8BufferFree(ref staleUtf8) == AfStatus.InvalidHandle &&
                BuffersEqual(staleUtf8, staleUtf8BeforeFree),
                "Failed UTF-8 status must leave no reusable owner.");
        }
        finally
        {
            ReleaseIfOwned(ref failedUtf8);
        }

        var f32Call = RenderVerticesRaw(cleanupSession);
        Require(
            f32Call.Status == AfStatus.Ok && f32Call.Buffer.Owner != 0,
            "F32 status-failure seam requires a real owner.");
        var failedF32 = f32Call.Buffer;
        var staleF32 = failedF32;
        try
        {
            var f32StatusError = ExpectThrows<InvalidOperationException>(
                () => cleanupSession.CopyAndFreeF32Result(
                    "synthetic_f32_status",
                    AfStatus.Internal,
                    ref failedF32),
                "Failed F32 status must propagate after cleanup.");
            Require(
                f32StatusError.Message.Contains("synthetic_f32_status", StringComparison.Ordinal) &&
                f32StatusError.Message.Contains("4", StringComparison.Ordinal),
                "F32 status error must remain primary.");
            RequireF32Empty(failedF32, "Failed F32 status must release and zero its owner.");
            var staleF32BeforeFree = staleF32;
            Require(
                AfNative.F32BufferFree(ref staleF32) == AfStatus.InvalidHandle &&
                F32BuffersEqual(staleF32, staleF32BeforeFree),
                "Failed F32 status must leave no reusable owner.");
        }
        finally
        {
            ReleaseF32IfOwned(ref failedF32);
        }

        var byteCall = SaveRaw(cleanupSession);
        Require(
            byteCall.Status == AfStatus.Ok && byteCall.Buffer.Owner != 0,
            "Byte status-failure seam requires a real owner.");
        var failedBytes = byteCall.Buffer;
        var staleBytes = failedBytes;
        try
        {
            var byteStatusError = ExpectThrows<InvalidOperationException>(
                () => cleanupSession.CopyAndFreeByteResult(
                    "synthetic_byte_status",
                    AfStatus.Internal,
                    ref failedBytes),
                "Failed byte status must propagate after cleanup.");
            Require(
                byteStatusError.Message.Contains("synthetic_byte_status", StringComparison.Ordinal) &&
                byteStatusError.Message.Contains("4", StringComparison.Ordinal),
                "Byte status error must remain primary.");
            RequireByteEmpty(failedBytes, "Failed byte status must release and zero its owner.");
            var staleBytesBeforeFree = staleBytes;
            Require(
                AfNative.ByteBufferFree(ref staleBytes) == AfStatus.InvalidHandle &&
                ByteBuffersEqual(staleBytes, staleBytesBeforeFree),
                "Failed byte status must leave no reusable owner.");
        }
        finally
        {
            ReleaseByteIfOwned(ref failedBytes);
        }

        var primaryCleanupUtf8Call = RenderFullRaw(cleanupSession);
        Require(
            primaryCleanupUtf8Call.Status == AfStatus.Ok &&
            primaryCleanupUtf8Call.Buffer.Owner != 0,
            "UTF-8 primary-plus-cleanup seam requires a real owner.");
        var primaryCleanupUtf8 = primaryCleanupUtf8Call.Buffer;
        var originalPrimaryCleanupUtf8 = primaryCleanupUtf8;
        var utf8RecoveryFreeCalls = 0;
        var utf8RecoveryStatus = AfStatus.Internal;
        try
        {
            primaryCleanupUtf8.Capacity = checked(primaryCleanupUtf8.Capacity + 1);
            var tamperedUtf8 = primaryCleanupUtf8;
            var utf8CleanupProbe = tamperedUtf8;
            Require(
                AfNative.Utf8BufferFree(ref utf8CleanupProbe) != AfStatus.Ok &&
                BuffersEqual(utf8CleanupProbe, tamperedUtf8),
                "Altered UTF-8 metadata must produce a cleanup failure without consuming its owner.");

            var utf8PrimaryError = ExpectThrows<InvalidOperationException>(
                () => cleanupSession.CopyAndFreeUtf8Result(
                    "synthetic_utf8_primary_cleanup",
                    AfStatus.Internal,
                    ref primaryCleanupUtf8),
                "UTF-8 primary status must survive a simultaneous cleanup failure.");
            Require(
                utf8PrimaryError.Message.Contains(
                    "synthetic_utf8_primary_cleanup",
                    StringComparison.Ordinal) &&
                utf8PrimaryError.Message.Contains("4", StringComparison.Ordinal) &&
                !utf8PrimaryError.Message.Contains(
                    nameof(AfNative.Utf8BufferFree),
                    StringComparison.Ordinal),
                "UTF-8 cleanup failure must not mask the primary status error.");
            Require(
                BuffersEqual(primaryCleanupUtf8, tamperedUtf8),
                "Rejected UTF-8 cleanup must leave the real owner recoverable.");
        }
        finally
        {
            if (primaryCleanupUtf8.Owner != 0)
            {
                primaryCleanupUtf8 = originalPrimaryCleanupUtf8;
                utf8RecoveryFreeCalls++;
                utf8RecoveryStatus = AfNative.Utf8BufferFree(ref primaryCleanupUtf8);
            }
        }

        Require(
            utf8RecoveryFreeCalls == 1 && utf8RecoveryStatus == AfStatus.Ok,
            "The recovered UTF-8 owner must be freed exactly once in finally.");
        RequireEmpty(primaryCleanupUtf8, "Recovered UTF-8 free must zero the buffer.");

        var primaryCleanupF32Call = RenderVerticesRaw(cleanupSession);
        Require(
            primaryCleanupF32Call.Status == AfStatus.Ok &&
            primaryCleanupF32Call.Buffer.Owner != 0,
            "F32 primary-plus-cleanup seam requires a real owner.");
        var primaryCleanupF32 = primaryCleanupF32Call.Buffer;
        var originalPrimaryCleanupF32 = primaryCleanupF32;
        var f32RecoveryFreeCalls = 0;
        var f32RecoveryStatus = AfStatus.Internal;
        try
        {
            primaryCleanupF32.Capacity = checked(primaryCleanupF32.Capacity + 1);
            var tamperedF32 = primaryCleanupF32;
            var f32CleanupProbe = tamperedF32;
            Require(
                AfNative.F32BufferFree(ref f32CleanupProbe) != AfStatus.Ok &&
                F32BuffersEqual(f32CleanupProbe, tamperedF32),
                "Altered F32 metadata must produce a cleanup failure without consuming its owner.");

            var f32PrimaryError = ExpectThrows<InvalidOperationException>(
                () => cleanupSession.CopyAndFreeF32Result(
                    "synthetic_f32_primary_cleanup",
                    AfStatus.Internal,
                    ref primaryCleanupF32),
                "F32 primary status must survive a simultaneous cleanup failure.");
            Require(
                f32PrimaryError.Message.Contains(
                    "synthetic_f32_primary_cleanup",
                    StringComparison.Ordinal) &&
                f32PrimaryError.Message.Contains("4", StringComparison.Ordinal) &&
                !f32PrimaryError.Message.Contains(
                    nameof(AfNative.F32BufferFree),
                    StringComparison.Ordinal),
                "F32 cleanup failure must not mask the primary status error.");
            Require(
                F32BuffersEqual(primaryCleanupF32, tamperedF32),
                "Rejected F32 cleanup must leave the real owner recoverable.");
        }
        finally
        {
            if (primaryCleanupF32.Owner != 0)
            {
                primaryCleanupF32 = originalPrimaryCleanupF32;
                f32RecoveryFreeCalls++;
                f32RecoveryStatus = AfNative.F32BufferFree(ref primaryCleanupF32);
            }
        }

        Require(
            f32RecoveryFreeCalls == 1 && f32RecoveryStatus == AfStatus.Ok,
            "The recovered F32 owner must be freed exactly once in finally.");
        RequireF32Empty(primaryCleanupF32, "Recovered F32 free must zero the buffer.");

        var primaryCleanupByteCall = SaveRaw(cleanupSession);
        Require(
            primaryCleanupByteCall.Status == AfStatus.Ok &&
            primaryCleanupByteCall.Buffer.Owner != 0,
            "Byte primary-plus-cleanup seam requires a real owner.");
        var primaryCleanupBytes = primaryCleanupByteCall.Buffer;
        var originalPrimaryCleanupBytes = primaryCleanupBytes;
        var byteRecoveryFreeCalls = 0;
        var byteRecoveryStatus = AfStatus.Internal;
        try
        {
            primaryCleanupBytes.Capacity = checked(primaryCleanupBytes.Capacity + 1);
            var tamperedBytes = primaryCleanupBytes;
            var byteCleanupProbe = tamperedBytes;
            Require(
                AfNative.ByteBufferFree(ref byteCleanupProbe) != AfStatus.Ok &&
                ByteBuffersEqual(byteCleanupProbe, tamperedBytes),
                "Altered byte metadata must produce a cleanup failure without consuming its owner.");

            var bytePrimaryError = ExpectThrows<InvalidOperationException>(
                () => cleanupSession.CopyAndFreeByteResult(
                    "synthetic_byte_primary_cleanup",
                    AfStatus.Internal,
                    ref primaryCleanupBytes),
                "Byte primary status must survive a simultaneous cleanup failure.");
            Require(
                bytePrimaryError.Message.Contains(
                    "synthetic_byte_primary_cleanup",
                    StringComparison.Ordinal) &&
                bytePrimaryError.Message.Contains("4", StringComparison.Ordinal) &&
                !bytePrimaryError.Message.Contains(
                    nameof(AfNative.ByteBufferFree),
                    StringComparison.Ordinal),
                "Byte cleanup failure must not mask the primary status error.");
            Require(
                ByteBuffersEqual(primaryCleanupBytes, tamperedBytes),
                "Rejected byte cleanup must leave the real owner recoverable.");
        }
        finally
        {
            if (primaryCleanupBytes.Owner != 0)
            {
                primaryCleanupBytes = originalPrimaryCleanupBytes;
                byteRecoveryFreeCalls++;
                byteRecoveryStatus = AfNative.ByteBufferFree(ref primaryCleanupBytes);
            }
        }

        Require(
            byteRecoveryFreeCalls == 1 && byteRecoveryStatus == AfStatus.Ok,
            "The recovered byte owner must be freed exactly once in finally.");
        RequireByteEmpty(primaryCleanupBytes, "Recovered byte free must zero the buffer.");

        cleanupSession.CreateLine(new ArcCadPoint(2, 2), new ArcCadPoint(3, 3));
        var failedDeltaCall = RenderDeltaRaw(cleanupSession);
        Require(
            failedDeltaCall.Status == AfStatus.Ok &&
            failedDeltaCall.Control.Owner != 0 &&
            failedDeltaCall.Vertices.Owner != 0,
            "Dual status-failure seam requires two real owners.");
        var failedControl = failedDeltaCall.Control;
        var failedVertices = failedDeltaCall.Vertices;
        var staleControl = failedControl;
        var staleFailedVertices = failedVertices;
        try
        {
            var dualStatusError = ExpectThrows<InvalidOperationException>(
                () => cleanupSession.CopyParseAndFreeRenderDelta(
                    AfStatus.Internal,
                    ref failedControl,
                    ref failedVertices),
                "Failed dual status must propagate after both cleanups.");
            Require(
                dualStatusError.Message.Contains(
                    nameof(AfNative.SessionRenderDelta),
                    StringComparison.Ordinal) &&
                dualStatusError.Message.Contains("4", StringComparison.Ordinal),
                "Dual status error must remain primary.");
            RequireEmpty(failedControl, "Failed dual status must release its UTF-8 owner.");
            RequireF32Empty(failedVertices, "Failed dual status must release its F32 owner.");
            Require(
                AfNative.Utf8BufferFree(ref staleControl) == AfStatus.InvalidHandle,
                "Failed dual status must invalidate its stale UTF-8 owner.");
            Require(
                AfNative.F32BufferFree(ref staleFailedVertices) == AfStatus.InvalidHandle,
                "Failed dual status must invalidate its stale F32 owner.");
        }
        finally
        {
            ReleaseIfOwned(ref failedControl);
            ReleaseF32IfOwned(ref failedVertices);
        }
    }

    var unknownJson = session.ExecuteJson("NOSUCHCOMMAND", "null");
    using (var unknownDocument = JsonDocument.Parse(unknownJson))
    {
        Require(
            unknownDocument.RootElement.GetProperty("error").GetProperty("code").GetString() ==
            "unknown_command",
            "Unknown command must return the unknown_command envelope.");
    }

    var malformedJson = session.ExecuteJson("LINE", "{not-json");
    using (var malformedDocument = JsonDocument.Parse(malformedJson))
    {
        Require(
            malformedDocument.RootElement.GetProperty("error").GetProperty("code").GetString() ==
            "malformed_json",
            "Malformed args must return the malformed_json envelope.");
    }

    const string unicodeCommand = "NO_EXISTE_ÁRBOL_東京";
    var unicodeJson = session.ExecuteJson(unicodeCommand, "null");
    using (var unicodeDocument = JsonDocument.Parse(unicodeJson))
    {
        var unicodeError = unicodeDocument.RootElement.GetProperty("error");
        Require(
            unicodeError.GetProperty("code").GetString() == "unknown_command",
            "Unicode command must remain a normal domain error.");
        Require(
            unicodeError.GetProperty("message").GetString()?.Contains(
                unicodeCommand,
                StringComparison.Ordinal) == true,
            "Unicode command must survive the UTF-8 round trip.");
    }

    ExpectThrows<EncoderFallbackException>(
        () => session.ExecuteJson("\uD800", "null"),
        "An isolated UTF-16 surrogate must be rejected before native execution.");
    using (var afterSurrogate = JsonDocument.Parse(session.ExecuteJson("NOSUCHCOMMAND", "null")))
    {
        Require(
            afterSurrogate.RootElement.GetProperty("error").GetProperty("code").GetString() ==
            "unknown_command",
            "Session must remain usable after UTF-16 encoding failure.");
    }

    Exception? wrongThreadExecuteError = null;
    Exception? wrongThreadLineError = null;
    Exception? wrongThreadParseError = null;
    Exception? wrongThreadSnapError = null;
    Exception? wrongThreadSelectError = null;
    Exception? wrongThreadUndoError = null;
    Exception? wrongThreadRedoError = null;
    Exception? wrongThreadDeltaError = null;
    Exception? wrongThreadRenderError = null;
    Exception? wrongThreadVerticesError = null;
    Exception? wrongThreadSaveError = null;
    Exception? wrongThreadOpenError = null;
    var wrongThreadExecute = new Thread(() =>
    {
        try
        {
            session.ExecuteJson("NOSUCHCOMMAND", "null");
        }
        catch (Exception exception)
        {
            wrongThreadExecuteError = exception;
        }

        try
        {
            session.CreateLine(new ArcCadPoint(0, 0), new ArcCadPoint(1, 1));
        }
        catch (Exception exception)
        {
            wrongThreadLineError = exception;
        }

        try
        {
            session.ParsePoint("1,2");
        }
        catch (Exception exception)
        {
            wrongThreadParseError = exception;
        }

        try
        {
            session.Snap(new ArcCadPoint(0, 0), 1);
        }
        catch (Exception exception)
        {
            wrongThreadSnapError = exception;
        }

        try
        {
            session.SelectAt(new ArcCadPoint(0, 0), 1);
        }
        catch (Exception exception)
        {
            wrongThreadSelectError = exception;
        }

        try
        {
            session.Undo();
        }
        catch (Exception exception)
        {
            wrongThreadUndoError = exception;
        }

        try
        {
            session.Redo();
        }
        catch (Exception exception)
        {
            wrongThreadRedoError = exception;
        }

        try
        {
            session.RenderDelta();
        }
        catch (Exception exception)
        {
            wrongThreadDeltaError = exception;
        }

        try
        {
            session.RenderFullJson();
        }
        catch (Exception exception)
        {
            wrongThreadRenderError = exception;
        }

        try
        {
            session.RenderVertices();
        }
        catch (Exception exception)
        {
            wrongThreadVerticesError = exception;
        }

        try
        {
            session.SaveArcf();
        }
        catch (Exception exception)
        {
            wrongThreadSaveError = exception;
        }

        try
        {
            session.OpenArcf(emptySavedBytes);
        }
        catch (Exception exception)
        {
            wrongThreadOpenError = exception;
        }
    });
    wrongThreadExecute.Start();
    wrongThreadExecute.Join();

    Require(
        wrongThreadExecuteError is InvalidOperationException,
        "Wrong-thread ExecuteJson must fail.");
    Require(
        wrongThreadLineError is InvalidOperationException,
        "Wrong-thread CreateLine must fail.");
    Require(
        wrongThreadParseError is InvalidOperationException,
        "Wrong-thread ParsePoint must fail.");
    Require(
        wrongThreadSnapError is InvalidOperationException,
        "Wrong-thread Snap must fail.");
    Require(
        wrongThreadSelectError is InvalidOperationException,
        "Wrong-thread SelectAt must fail.");
    Require(
        wrongThreadUndoError is InvalidOperationException,
        "Wrong-thread Undo must fail.");
    Require(
        wrongThreadRedoError is InvalidOperationException,
        "Wrong-thread Redo must fail.");
    Require(
        wrongThreadDeltaError is InvalidOperationException,
        "Wrong-thread RenderDelta must fail.");
    Require(
        wrongThreadRenderError is InvalidOperationException,
        "Wrong-thread RenderFullJson must fail.");
    Require(
        wrongThreadVerticesError is InvalidOperationException,
        "Wrong-thread RenderVertices must fail.");
    Require(
        wrongThreadSaveError is InvalidOperationException,
        "Wrong-thread SaveArcf must fail.");
    Require(
        wrongThreadOpenError is InvalidOperationException,
        "Wrong-thread OpenArcf must fail.");
    Require(
        session.DangerousHandle == handle,
        "Wrong-thread calls must preserve the handle.");
    var verticesAfterWrongThread = session.RenderVertices();
    Require(verticesAfterWrongThread.Length >= 4, "Owner thread must remain able to render vertices.");
    RequireFloatBits(
        verticesAfterWrongThread.Take(4).ToArray(),
        [0f, 0f, 10f, 20f],
        "Wrong-thread attempt must not alter the owner-thread vertex buffer.");
    using (var renderAfterWrongThread = JsonDocument.Parse(session.RenderFullJson()))
    {
        RequireExactProperties(
            renderAfterWrongThread.RootElement,
            "batches",
            "vertices",
            "ltscale");
    }
    using (var afterWrongThread = JsonDocument.Parse(session.ExecuteJson("NOSUCHCOMMAND", "null")))
    {
        Require(
            afterWrongThread.RootElement.GetProperty("error").GetProperty("code").GetString() ==
            "unknown_command",
            "Owner thread must remain able to execute after a wrong-thread attempt.");
    }

    nint invalidUtf8Pointer = nint.Zero;
    AfUtf8BufferNative invalidUtf8Result = default;
    try
    {
        invalidUtf8Pointer = Marshal.AllocHGlobal(1);
        Marshal.WriteByte(invalidUtf8Pointer, 0, 0xff);
        var invalidUtf8Call = ExecuteRaw(
            session,
            invalidUtf8Pointer,
            1,
            nint.Zero,
            0);
        var invalidUtf8Status = invalidUtf8Call.Status;
        invalidUtf8Result = invalidUtf8Call.Buffer;
        Require(
            invalidUtf8Status == AfStatus.InvalidUtf8,
            $"Invalid command UTF-8 returned {(uint)invalidUtf8Status}.");
        RequireEmpty(invalidUtf8Result, "Invalid UTF-8 output must be canonical empty.");
    }
    finally
    {
        try
        {
            ReleaseIfOwned(ref invalidUtf8Result);
        }
        finally
        {
            if (invalidUtf8Pointer != nint.Zero)
            {
                Marshal.FreeHGlobal(invalidUtf8Pointer);
            }
        }
    }

    nint rawCommand = nint.Zero;
    nint rawArgs = nint.Zero;
    AfUtf8BufferNative rawResult = default;
    try
    {
        rawCommand = AllocateUtf8("NOSUCHCOMMAND", out var rawCommandLength);
        rawArgs = AllocateUtf8("null", out var rawArgsLength);
        var rawCall = ExecuteRaw(
            session,
            rawCommand,
            rawCommandLength,
            rawArgs,
            rawArgsLength);
        var rawStatus = rawCall.Status;
        rawResult = rawCall.Buffer;
        Require(rawStatus == AfStatus.Ok, $"Raw execute returned {(uint)rawStatus}.");

        var stale = rawResult;
        using (var rawDocument = JsonDocument.Parse(CopyUtf8(rawResult)))
        {
            Require(
                rawDocument.RootElement.GetProperty("error").GetProperty("code").GetString() ==
                "unknown_command",
                "Raw execute must return a copyable JSON envelope.");
        }

        var freeStatus = AfNative.Utf8BufferFree(ref rawResult);
        Require(freeStatus == AfStatus.Ok, $"Raw free returned {(uint)freeStatus}.");
        RequireEmpty(rawResult, "Successful free must zero all buffer fields.");
        Require(
            AfNative.Utf8BufferFree(ref rawResult) == AfStatus.Ok,
            "Free of canonical empty buffer must be idempotent.");

        var staleBeforeFree = stale;
        Require(
            AfNative.Utf8BufferFree(ref stale) == AfStatus.InvalidHandle,
            "Free of stale owner must return INVALID_HANDLE.");
        Require(
            BuffersEqual(stale, staleBeforeFree),
            "Failed stale free must leave all metadata unchanged.");
    }
    finally
    {
        try
        {
            ReleaseIfOwned(ref rawResult);
        }
        finally
        {
            try
            {
                if (rawCommand != nint.Zero)
                {
                    Marshal.FreeHGlobal(rawCommand);
                }
            }
            finally
            {
                if (rawArgs != nint.Zero)
                {
                    Marshal.FreeHGlobal(rawArgs);
                }
            }
        }
    }

    Exception? wrongThreadDisposeError = null;
    var wrongThreadDispose = new Thread(() =>
    {
        try
        {
            session.Dispose();
        }
        catch (Exception exception)
        {
            wrongThreadDisposeError = exception;
        }
    });
    wrongThreadDispose.Start();
    wrongThreadDispose.Join();

    Require(
        wrongThreadDisposeError is InvalidOperationException,
        "Wrong-thread Dispose must fail.");
    Require(session.DangerousHandle == handle, "Wrong-thread Dispose must preserve the handle.");

    session.Dispose();
    Require(session.IsDisposed, "Owner-thread Dispose must close the session.");
    ExpectThrows<ObjectDisposedException>(
        () => session.ExecuteJson("NOSUCHCOMMAND", "null"),
        "ExecuteJson after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.CreateLine(new ArcCadPoint(0, 0), new ArcCadPoint(1, 1)),
        "CreateLine after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.ParsePoint("1,2"),
        "ParsePoint after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.Snap(new ArcCadPoint(0, 0), 1),
        "Snap after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.SelectAt(new ArcCadPoint(0, 0), 1),
        "SelectAt after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.Undo(),
        "Undo after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.Redo(),
        "Redo after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.RenderDelta(),
        "RenderDelta after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.RenderFullJson(),
        "RenderFullJson after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.RenderVertices(),
        "RenderVertices after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.SaveArcf(),
        "SaveArcf after Dispose must fail.");
    ExpectThrows<ObjectDisposedException>(
        () => session.OpenArcf(emptySavedBytes),
        "OpenArcf after Dispose must fail.");
    RequireFloatBits(
        copiedLineVertices,
        [0f, 0f, 10f, 20f],
        "Managed vertices must survive session disposal.");
    Require(
        lineDelta.Upserts.Length == 1 &&
        lineDelta.Upserts.Span[0].Strips.Length == 1 &&
        lineDelta.Upserts.Span[0].Strips.Span[0].EntityId == renderedLine.EntityId,
        "Managed delta control data must survive session disposal.");
    RequireFloatBits(
        lineDelta.Vertices.ToArray(),
        [0f, 0f, 10f, 20f],
        "Managed delta vertices must survive session disposal.");
    session.Dispose();
    Require(session.IsDisposed, "Repeated Dispose must be idempotent.");
    Require(
        AfNative.SessionDestroy(handle) == AfStatus.InvalidHandle,
        "Destroy after Dispose must return INVALID_HANDLE (2).");
}
finally
{
    if (!session.IsDisposed)
    {
        session.Dispose();
    }
}

var rectangleIds = new ulong[4];
var rectangleSegments = new[]
{
    (Start: new ArcCadPoint(0, 0), End: new ArcCadPoint(10, 0)),
    (Start: new ArcCadPoint(10, 0), End: new ArcCadPoint(10, 10)),
    (Start: new ArcCadPoint(10, 10), End: new ArcCadPoint(0, 10)),
    (Start: new ArcCadPoint(0, 10), End: new ArcCadPoint(0, 0)),
};
byte[] arcfBytes;
using (var persistenceSession = ArcCadSession.Create())
{
    for (var index = 0; index < rectangleSegments.Length; index++)
    {
        var segment = rectangleSegments[index];
        var line = persistenceSession.CreateLine(segment.Start, segment.End);
        Require(
            line.TransactionSequence == (ulong)index,
            "Rectangle LINE transaction sequences must be consecutive.");
        rectangleIds[index] = line.EntityId;
    }

    Require(
        rectangleIds.All(id => id != 0) && rectangleIds.Distinct().Count() == 4,
        "Rectangle LINE ids must be nonzero and unique.");
    arcfBytes = persistenceSession.SaveArcf();
    Require(arcfBytes.Length > 0, "Managed save must return non-empty ARCF bytes.");

    AfByteBufferNative rawSavedBytes = default;
    try
    {
        var rawSave = SaveRaw(persistenceSession);
        Require(rawSave.Status == AfStatus.Ok, $"Raw save returned {(uint)rawSave.Status}.");
        rawSavedBytes = rawSave.Buffer;
        var rawCopy = ArcCadSession.CopyByteResult(
            nameof(AfNative.SessionSaveArcf),
            rawSavedBytes);
        Require(rawCopy.Length > 0, "Raw save bytes must remain copyable before free.");
        arcfBytes = rawCopy;

        var wrongType = new AfUtf8BufferNative
        {
            Data = rawSavedBytes.Data,
            Length = rawSavedBytes.Length,
            Capacity = rawSavedBytes.Capacity,
            Owner = rawSavedBytes.Owner,
        };
        var wrongTypeBeforeFree = wrongType;
        Require(
            AfNative.Utf8BufferFree(ref wrongType) != AfStatus.Ok,
            "A byte owner must not be releasable through the UTF-8 registry.");
        Require(
            BuffersEqual(wrongType, wrongTypeBeforeFree),
            "Wrong-type free must leave byte-owner metadata unchanged.");

        var tamperedBytes = rawSavedBytes;
        tamperedBytes.Capacity = checked(tamperedBytes.Capacity + 1);
        var tamperedBeforeFree = tamperedBytes;
        Require(
            AfNative.ByteBufferFree(ref tamperedBytes) != AfStatus.Ok,
            "Altered byte metadata must be rejected.");
        Require(
            ByteBuffersEqual(tamperedBytes, tamperedBeforeFree),
            "Rejected byte free must leave altered metadata unchanged.");

        var staleBytes = rawSavedBytes;
        Require(
            AfNative.ByteBufferFree(ref rawSavedBytes) == AfStatus.Ok,
            "Raw byte free must succeed.");
        RequireByteEmpty(rawSavedBytes, "Successful byte free must zero all fields.");
        Require(
            AfNative.ByteBufferFree(ref rawSavedBytes) == AfStatus.Ok,
            "Free of canonical empty byte buffer must be idempotent.");
        var staleBeforeFree = staleBytes;
        Require(
            AfNative.ByteBufferFree(ref staleBytes) == AfStatus.InvalidHandle,
            "Free of a stale byte owner must return INVALID_HANDLE.");
        Require(
            ByteBuffersEqual(staleBytes, staleBeforeFree),
            "Failed stale byte free must leave metadata unchanged.");
    }
    finally
    {
        ReleaseByteIfOwned(ref rawSavedBytes);
    }
}

using (var reopenedSession = ArcCadSession.Create())
{
    var warnings = reopenedSession.OpenArcf(arcfBytes);
    Require(
        warnings.Count == 0 && warnings is ICollection<string> { IsReadOnly: true },
        "A native save/open roundtrip must return a read-only empty warning list.");
    RequireRectangleDelta(
        reopenedSession.RenderDelta(),
        rectangleIds,
        "Reopened rectangle");

    var lineAfterOpen = reopenedSession.CreateLine(
        new ArcCadPoint(20, 20),
        new ArcCadPoint(30, 30));
    Require(
        lineAfterOpen.TransactionSequence == 0,
        "The first command after open must restart at txSeq 0.");
    Require(
        lineAfterOpen.EntityId == checked(rectangleIds.Max() + 1),
        "The first LINE after open must use the persisted next entity id.");
}

using (var corruptSession = ArcCadSession.Create())
{
    var preservedLine = corruptSession.CreateLine(
        new ArcCadPoint(-1, -2),
        new ArcCadPoint(3, 4));
    var beforeCorruptOpen = corruptSession.SaveArcf();
    var corruptError = ExpectThrows<ArcCadCommandException>(
        () => corruptSession.OpenArcf(Encoding.ASCII.GetBytes("not-an-arcf")),
        "Corrupt ARCF bytes must return a structured product error.");
    Require(
        !string.IsNullOrEmpty(corruptError.Code) && !string.IsNullOrEmpty(corruptError.Message),
        "Corrupt ARCF errors must preserve code and message.");
    Require(!corruptSession.IsDisposed, "A corrupt document must not terminate the session.");
    Require(
        corruptSession.SaveArcf().SequenceEqual(beforeCorruptOpen),
        "A corrupt open must preserve the previous document byte-for-byte.");
    var lineAfterCorruptOpen = corruptSession.CreateLine(
        new ArcCadPoint(5, 6),
        new ArcCadPoint(7, 8));
    Require(
        lineAfterCorruptOpen.TransactionSequence == 1 &&
        lineAfterCorruptOpen.EntityId == checked(preservedLine.EntityId + 1),
        "A corrupt open must preserve transaction and entity-id state.");
}

var panicSession = ArcCadSession.Create();
var panicHandle = panicSession.DangerousHandle;
try
{
    var panicCall = RenderFullRaw(panicSession);
    Require(
        panicCall.Status == AfStatus.Ok && panicCall.Buffer.Owner != 0,
        "PANIC cleanup seam requires a real UTF-8 owner.");
    var panicBuffer = panicCall.Buffer;
    var stalePanicBuffer = panicBuffer;
    InvalidOperationException panicError;
    try
    {
        panicError = ExpectThrows<InvalidOperationException>(
            () => panicSession.CopyAndFreeUtf8Result(
                "synthetic_panic",
                AfStatus.Panic,
                ref panicBuffer),
            "PANIC status must be propagated after discarding the session.");
        RequireEmpty(panicBuffer, "PANIC status must release and zero its native owner.");
        Require(
            AfNative.Utf8BufferFree(ref stalePanicBuffer) == AfStatus.InvalidHandle,
            "PANIC status must leave no reusable UTF-8 owner.");
    }
    finally
    {
        ReleaseIfOwned(ref panicBuffer);
    }

    Require(
        panicError.Message.Contains("synthetic_panic", StringComparison.Ordinal) &&
        panicError.Message.Contains("255", StringComparison.Ordinal),
        "PANIC exception must retain operation and status.");
    Require(panicSession.IsDisposed, "PANIC status must close the managed session handle.");
    ExpectThrows<ObjectDisposedException>(
        () => panicSession.RenderDelta(),
        "A session discarded after PANIC must reject reuse.");
    ExpectThrows<ObjectDisposedException>(
        () => panicSession.SaveArcf(),
        "A session discarded after PANIC must reject save.");
    ExpectThrows<ObjectDisposedException>(
        () => panicSession.OpenArcf(arcfBytes),
        "A session discarded after PANIC must reject open.");
    Require(
        AfNative.SessionDestroy(panicHandle) == AfStatus.InvalidHandle,
        "The discarded PANIC session must be terminal in native code.");
}
finally
{
    if (!panicSession.IsDisposed)
    {
        panicSession.Dispose();
    }
}

var finalizedHandle = CreateAbandonedSession();
GC.Collect();
GC.WaitForPendingFinalizers();
GC.Collect();
Require(
    AfNativeSessionWorker.Invoke(() => AfNative.SessionDestroy(finalizedHandle)) ==
    AfStatus.InvalidHandle,
    "SafeHandle finalization must destroy the abandoned native session.");
using (var sessionAfterFinalizer = ArcCadSession.Create())
{
    Require(
        sessionAfterFinalizer.RenderVertices().Length == 0,
        "The native worker must remain usable after SafeHandle finalization.");
}

var verticalLineId = 0UL;
var verticalLineBytes = Array.Empty<byte>();
var verticalLineSession = ArcCadSession.Create();
try
{
    var verticalLineStart = verticalLineSession.ParsePoint("0,0");
    var verticalLineEnd = verticalLineSession.ParsePoint("@10,20", verticalLineStart);
    Require(
        verticalLineStart == new ArcCadPoint(0, 0) &&
        verticalLineEnd == new ArcCadPoint(10, 20),
        "Vertical LINE parsed points must preserve absolute and relative input.");

    var verticalLine = verticalLineSession.CreateLine(verticalLineStart, verticalLineEnd);
    verticalLineId = verticalLine.EntityId;
    Require(
        verticalLine.TransactionSequence == 0 && verticalLineId != 0,
        "Vertical LINE must be the first transaction and return a nonzero id.");

    var verticalLineDelta = verticalLineSession.RenderDelta();
    RequireDeltaEntities(verticalLineDelta, [verticalLineId], "Vertical LINE delta");
    Require(
        verticalLineDelta.Upserts.Length == 1 && verticalLineDelta.LinetypeScale == 1.0,
        "Vertical LINE delta must contain one batch and preserve LTSCALE.");
    var verticalLineBatch = verticalLineDelta.Upserts.Span[0];
    Require(
        verticalLineBatch.Strips.Length == 1 && verticalLineBatch.Markers.Length == 0,
        "Vertical LINE delta must contain one strip and no markers.");
    var verticalLineStrip = verticalLineBatch.Strips.Span[0];
    Require(
        verticalLineStrip is
        {
            EntityId: var renderedVerticalLineId,
            Offset: 0,
            Count: 2,
            Width: 0.25f,
            PolyWidth: 0,
            AnalyticLength: var verticalLineLength,
        } &&
        renderedVerticalLineId == verticalLineId &&
        verticalLineLength is { } analyticLength &&
        Math.Abs(analyticLength - Math.Sqrt(500.0)) <= 1e-12,
        "Vertical LINE typed render must preserve id and exact strip metadata.");
    RequireFloatBits(
        verticalLineDelta.Vertices.ToArray(),
        [0f, 0f, 10f, 20f],
        "Vertical LINE typed render must preserve geometry bit-exactly.");
    RequireSingleLineRender(verticalLineSession.RenderFullJson(), verticalLineId);

    var verticalLineHit = verticalLineSession.SelectAt(new ArcCadPoint(5, 10), 0.5);
    Require(
        verticalLineHit.Count == 1 && verticalLineHit[0] == verticalLineId,
        "Vertical LINE selection hit must return exactly its id.");
    Require(
        verticalLineSession.SelectAt(new ArcCadPoint(100, 100), 0.5).Count == 0,
        "Vertical LINE selection miss must be empty.");

    verticalLineSession.Undo();
    Require(
        verticalLineSession.RenderVertices().Length == 0,
        "Vertical LINE undo must leave empty render geometry.");
    using (var undoneVerticalLineRender = JsonDocument.Parse(verticalLineSession.RenderFullJson()))
    {
        var undoneRoot = undoneVerticalLineRender.RootElement;
        RequireExactProperties(undoneRoot, "batches", "vertices", "ltscale");
        Require(
            undoneRoot.GetProperty("batches").GetArrayLength() == 0 &&
            undoneRoot.GetProperty("vertices").GetArrayLength() == 0 &&
            undoneRoot.GetProperty("ltscale").GetDouble() == 1.0,
            "Vertical LINE undo must leave the scene empty.");
    }
    Require(
        verticalLineSession.SelectAt(new ArcCadPoint(5, 10), 0.5).Count == 0,
        "Vertical LINE undo must leave selection empty.");

    verticalLineSession.Redo();
    var redoneVerticalLineDelta = verticalLineSession.RenderDelta();
    RequireDeltaEntities(redoneVerticalLineDelta, [verticalLineId], "Redone vertical LINE delta");
    RequireFloatBits(
        redoneVerticalLineDelta.Vertices.ToArray(),
        [0f, 0f, 10f, 20f],
        "Vertical LINE redo must restore geometry bit-exactly.");
    RequireSingleLineRender(verticalLineSession.RenderFullJson(), verticalLineId);
    var redoneVerticalLineHit = verticalLineSession.SelectAt(new ArcCadPoint(5, 10), 0.5);
    Require(
        redoneVerticalLineHit.Count == 1 && redoneVerticalLineHit[0] == verticalLineId,
        "Vertical LINE redo must restore the same selectable id.");

    var sequenceProbeLine = verticalLineSession.CreateLine(
        new ArcCadPoint(1000, 1000),
        new ArcCadPoint(1010, 1020));
    Require(
        sequenceProbeLine.TransactionSequence == 1 &&
        sequenceProbeLine.EntityId != 0 &&
        sequenceProbeLine.EntityId != verticalLineId,
        "Vertical LINE queries and undo/redo must leave the next transaction at sequence 1.");

    verticalLineSession.Undo();
    RequireFloatBits(
        verticalLineSession.RenderVertices(),
        [0f, 0f, 10f, 20f],
        "Sequence-probe undo must restore only the vertical LINE geometry.");
    RequireSingleLineRender(verticalLineSession.RenderFullJson(), verticalLineId);
    Require(
        verticalLineSession.SelectAt(new ArcCadPoint(1005, 1010), 0.5).Count == 0,
        "Undone sequence-probe LINE must not remain selectable.");
    var verticalLineAfterProbeHit = verticalLineSession.SelectAt(new ArcCadPoint(5, 10), 0.5);
    Require(
        verticalLineAfterProbeHit.Count == 1 &&
        verticalLineAfterProbeHit[0] == verticalLineId,
        "Sequence-probe undo must leave only the vertical LINE selectable.");

    verticalLineBytes = verticalLineSession.SaveArcf();
    Require(verticalLineBytes.Length > 0, "Vertical LINE save must produce non-empty bytes.");
}
finally
{
    verticalLineSession.Dispose();
}
Require(verticalLineSession.IsDisposed, "Vertical LINE source session must be closed before reopen.");

using (var reopenedVerticalLineSession = ArcCadSession.Create())
{
    var verticalLineWarnings = reopenedVerticalLineSession.OpenArcf(verticalLineBytes);
    Require(verticalLineWarnings.Count == 0, "Vertical LINE reopen must return no warnings.");

    var reopenedVerticalLineDelta = reopenedVerticalLineSession.RenderDelta();
    RequireDeltaEntities(
        reopenedVerticalLineDelta,
        [verticalLineId],
        "Reopened vertical LINE delta");
    RequireFloatBits(
        reopenedVerticalLineDelta.Vertices.ToArray(),
        [0f, 0f, 10f, 20f],
        "Vertical LINE reopen must preserve geometry bit-exactly.");
    RequireSingleLineRender(reopenedVerticalLineSession.RenderFullJson(), verticalLineId);

    var reopenedVerticalLineHit = reopenedVerticalLineSession.SelectAt(
        new ArcCadPoint(5, 10),
        0.5);
    Require(
        reopenedVerticalLineHit.Count == 1 && reopenedVerticalLineHit[0] == verticalLineId,
        "Vertical LINE reopen must preserve the same selectable id.");
    Require(
        reopenedVerticalLineSession.SelectAt(new ArcCadPoint(100, 100), 0.5).Count == 0,
        "Reopened vertical LINE selection miss must remain empty.");

    var reopenedVerticalLineUndoError = ExpectThrows<ArcCadCommandException>(
        () => reopenedVerticalLineSession.Undo(),
        "Vertical LINE reopen must reset session history.");
    Require(
        reopenedVerticalLineUndoError.Code == "nothing_to_undo",
        "Vertical LINE reopened history must report nothing_to_undo.");
}

Console.WriteLine(
    $"PASS ABI={nativeVersion.Major}.{nativeVersion.Minor}.{nativeVersion.Patch} " +
    $"HANDLE={handle} EXECUTE_JSON=PASS CREATE_LINE=PASS " +
    $"PARSE_POINT=PASS SNAP=PASS SELECT_AT=PASS UNDO=PASS REDO=PASS " +
    $"SAVE_ARCF=PASS OPEN_ARCF=PASS " +
    $"RENDER_DELTA=PASS " +
    $"RENDER_FULL=PASS RENDER_VERTICES=PASS " +
    $"FAILED_OWNER_CLEANUP=PASS MISSING_APP_LOCAL_DLL=PASS LINE_VERTICAL=PASS");
Console.WriteLine($"af_ffi.dll={loadedDllPath}");
Console.WriteLine($"sha256={loadedDllHash}");
Console.WriteLine($"libunwind.dll={loadedUnwindPath}");

static string HashFile(string path)
{
    using var stream = File.OpenRead(path);
    return Convert.ToHexString(SHA256.HashData(stream)).ToLowerInvariant();
}

static void RunMissingDllProbe(string expectedDllPath, string expectedUnwindPath)
{
    var probeParent = Path.GetFullPath(Path.Combine(
        Path.GetTempPath(),
        "arccad-missing-native-probe"));
    var probeRoot = Path.Combine(probeParent, Guid.NewGuid().ToString("N"));
    var appDirectory = Path.Combine(probeRoot, "app");
    var decoyDirectory = Path.Combine(probeRoot, "decoy");
    Directory.CreateDirectory(appDirectory);
    Directory.CreateDirectory(decoyDirectory);

    try
    {
        foreach (var sourcePath in Directory.EnumerateFiles(AppContext.BaseDirectory))
        {
            var fileName = Path.GetFileName(sourcePath);
            if (string.Equals(fileName, "af_ffi.dll", StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            File.Copy(sourcePath, Path.Combine(appDirectory, fileName));
        }

        File.Copy(expectedDllPath, Path.Combine(decoyDirectory, "af_ffi.dll"));
        File.Copy(expectedUnwindPath, Path.Combine(decoyDirectory, "libunwind.dll"));

        var entryAssemblyPath = Assembly.GetEntryAssembly()?.Location ??
            throw new InvalidOperationException("Smoke entry assembly path is unavailable.");
        var copiedEntryAssembly = Path.Combine(appDirectory, Path.GetFileName(entryAssemblyPath));
        var sourceAppHost = Path.ChangeExtension(entryAssemblyPath, ".exe");
        var startInfo = new ProcessStartInfo
        {
            FileName = File.Exists(sourceAppHost)
                ? Path.Combine(appDirectory, Path.GetFileName(sourceAppHost))
                : Environment.ProcessPath ??
                    throw new InvalidOperationException(".NET host path is unavailable."),
            WorkingDirectory = decoyDirectory,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };
        if (!File.Exists(sourceAppHost))
        {
            startInfo.ArgumentList.Add(copiedEntryAssembly);
        }

        startInfo.ArgumentList.Add("--expect-missing-app-local");
        var inheritedPath = Environment.GetEnvironmentVariable("PATH");
        startInfo.Environment["PATH"] = string.IsNullOrEmpty(inheritedPath)
            ? decoyDirectory
            : decoyDirectory + Path.PathSeparator + inheritedPath;

        using var child = Process.Start(startInfo) ??
            throw new InvalidOperationException("Missing-DLL probe process did not start.");
        var standardOutput = child.StandardOutput.ReadToEndAsync();
        var standardError = child.StandardError.ReadToEndAsync();
        var exited = child.WaitForExit(30_000);
        if (!exited)
        {
            child.Kill(entireProcessTree: true);
            child.WaitForExit();
        }

        var output = standardOutput.GetAwaiter().GetResult();
        var error = standardError.GetAwaiter().GetResult();
        Require(exited, "Missing-DLL probe timed out.");
        Require(
            child.ExitCode == 0 &&
            output.Contains("PASS MISSING_APP_LOCAL_DLL", StringComparison.Ordinal),
            $"Missing-DLL probe failed. exit={child.ExitCode} stdout={output} stderr={error}");
    }
    finally
    {
        if (Directory.Exists(probeRoot))
        {
            Directory.Delete(probeRoot, recursive: true);
        }
    }
}

[MethodImpl(MethodImplOptions.NoInlining)]
static nuint CreateAbandonedSession()
{
    var session = ArcCadSession.Create();
    return session.DangerousHandle;
}

static (AfStatus Status, AfByteBufferNative Buffer) SaveRaw(ArcCadSession session) =>
    AfNativeSessionWorker.Invoke(() =>
    {
        var status = AfNative.SessionSaveArcf(session.SafeHandle, out var buffer);
        return (status, buffer);
    });

static (AfStatus Status, AfUtf8BufferNative Buffer) RenderFullRaw(ArcCadSession session) =>
    AfNativeSessionWorker.Invoke(() =>
    {
        var status = AfNative.SessionRenderFullJson(session.SafeHandle, out var buffer);
        return (status, buffer);
    });

static (AfStatus Status, AfF32BufferNative Buffer) RenderVerticesRaw(ArcCadSession session) =>
    AfNativeSessionWorker.Invoke(() =>
    {
        var status = AfNative.SessionRenderVertices(session.SafeHandle, out var buffer);
        return (status, buffer);
    });

static (AfStatus Status, AfUtf8BufferNative Control, AfF32BufferNative Vertices) RenderDeltaRaw(
    ArcCadSession session) =>
    AfNativeSessionWorker.Invoke(() =>
    {
        var status = AfNative.SessionRenderDelta(
            session.SafeHandle,
            out var control,
            out var vertices);
        return (status, control, vertices);
    });

static (AfStatus Status, AfUtf8BufferNative Buffer) ExecuteRaw(
    ArcCadSession session,
    nint command,
    nuint commandLength,
    nint argsJson,
    nuint argsJsonLength) =>
    AfNativeSessionWorker.Invoke(() =>
    {
        var status = AfNative.SessionExecuteJson(
            session.SafeHandle,
            command,
            commandLength,
            argsJson,
            argsJsonLength,
            out var buffer);
        return (status, buffer);
    });

static void RequireDeltaEntities(
    ArcCadRenderDelta delta,
    ulong[] expectedEntityIds,
    string message)
{
    var actualEntityIds = new List<ulong>();
    foreach (var batch in delta.Upserts.Span)
    {
        foreach (var strip in batch.Strips.Span)
        {
            Require(strip.Count == 2, $"{message} strips must contain two points.");
            actualEntityIds.Add(strip.EntityId);
        }
    }

    actualEntityIds.Sort();
    var expected = expectedEntityIds.Order().ToArray();
    Require(delta.Removes.Length == 0, $"{message} must not remove the LINE batch.");
    Require(
        delta.Vertices.Length == checked(expected.Length * 4),
        $"{message} must contain exactly two points per LINE.");
    Require(
        actualEntityIds.SequenceEqual(expected),
        $"{message} entity ids do not match.");
}

static void RequireRectangleDelta(
    ArcCadRenderDelta delta,
    ulong[] expectedEntityIds,
    string message)
{
    Require(expectedEntityIds.Length == 4, $"{message} requires four entity ids.");
    Require(
        delta.Upserts.Length == 1 &&
        delta.Removes.Length == 0 &&
        delta.Vertices.Length == 16 &&
        delta.LinetypeScale == 1.0,
        $"{message} delta shape is invalid.");

    var batch = delta.Upserts.Span[0];
    Require(
        batch.Strips.Length == 4 && batch.Markers.Length == 0,
        $"{message} must contain four LINE strips and no markers.");
    for (var index = 0; index < expectedEntityIds.Length; index++)
    {
        var strip = batch.Strips.Span[index];
        Require(
            strip.EntityId == expectedEntityIds[index] &&
            strip.Offset == (uint)(index * 2) &&
            strip.Count == 2,
            $"{message} must preserve LINE id and order.");
    }

    RequireFloatBits(
        delta.Vertices.ToArray(),
        [
            0f, 0f, 10f, 0f,
            10f, 0f, 10f, 10f,
            10f, 10f, 0f, 10f,
            0f, 10f, 0f, 0f,
        ],
        $"{message} must preserve rectangle geometry bit-exactly.");
}

static void RequireSingleLineRender(string json, ulong entityId)
{
    using var document = JsonDocument.Parse(json);
    var root = document.RootElement;
    RequireExactProperties(root, "batches", "vertices", "ltscale");

    var batches = root.GetProperty("batches");
    Require(batches.GetArrayLength() == 1, "LINE render must contain exactly one batch.");
    var batchEnumerator = batches.EnumerateArray();
    Require(batchEnumerator.MoveNext(), "LINE render batch is missing.");
    var batch = batchEnumerator.Current;
    RequireExactProperties(batch, "layer", "color", "linetype", "strips", "markers");
    Require(batch.GetProperty("layer").TryGetUInt64(out _), "Render layer must be an opaque u64.");
    Require(
        batch.GetProperty("linetype").TryGetUInt64(out _),
        "Render linetype must be an opaque u64.");

    var color = batch.GetProperty("color").EnumerateArray()
        .Select(component => component.GetInt32())
        .ToArray();
    Require(
        color.SequenceEqual(new[] { 255, 255, 255, 255 }),
        "Default LINE render color must be opaque white.");
    Require(batch.GetProperty("markers").GetArrayLength() == 0, "LINE render must have no markers.");

    var strips = batch.GetProperty("strips");
    Require(strips.GetArrayLength() == 1, "LINE render must contain exactly one strip.");
    var stripEnumerator = strips.EnumerateArray();
    Require(stripEnumerator.MoveNext(), "LINE render strip is missing.");
    var strip = stripEnumerator.Current;
    RequireExactProperties(
        strip,
        "entity",
        "offset",
        "count",
        "width",
        "polyWidth",
        "analyticLength");
    Require(strip.GetProperty("entity").GetUInt64() == entityId, "Render entity id must match LINE.");
    Require(strip.GetProperty("offset").GetUInt32() == 0, "LINE strip offset must be zero.");
    Require(strip.GetProperty("count").GetUInt32() == 2, "LINE strip must contain two points.");
    Require(strip.GetProperty("width").GetSingle() == 0.25f, "LINE strip width must be 0.25 mm.");
    var rawPolyWidth = strip.GetProperty("polyWidth");
    Require(
        rawPolyWidth.TryGetSingle(out var observedPolyWidth) && observedPolyWidth == 0,
        $"LINE strip polyWidth must be zero; observed {rawPolyWidth.GetRawText()}.");
    var rawAnalyticLength = strip.GetProperty("analyticLength");
    Require(
        rawAnalyticLength.TryGetDouble(out var observedAnalyticLength) &&
        Math.Abs(observedAnalyticLength - Math.Sqrt(500.0)) <= 1e-12,
        $"LINE strip analyticLength must equal sqrt(500); observed {rawAnalyticLength.GetRawText()}.");

    var vertices = root.GetProperty("vertices").EnumerateArray()
        .Select(vertex => vertex.GetSingle())
        .ToArray();
    Require(
        vertices.SequenceEqual(new[] { 0f, 0f, 10f, 20f }),
        "LINE render vertices must preserve both endpoints.");
    Require(root.GetProperty("ltscale").GetDouble() == 1.0, "LINE render LTSCALE must be 1.");
}

static void RequireExactProperties(JsonElement element, params string[] expectedNames)
{
    Require(element.ValueKind == JsonValueKind.Object, "Expected a JSON object.");
    var actual = element.EnumerateObject()
        .Select(property => property.Name)
        .OrderBy(name => name, StringComparer.Ordinal)
        .ToArray();
    var expected = expectedNames
        .OrderBy(name => name, StringComparer.Ordinal)
        .ToArray();
    Require(
        actual.SequenceEqual(expected),
        $"JSON properties mismatch: expected [{string.Join(',', expected)}], " +
        $"got [{string.Join(',', actual)}].");
}

static nint AllocateUtf8(string value, out nuint length)
{
    var bytes = Encoding.UTF8.GetBytes(value);
    length = (nuint)bytes.Length;
    if (bytes.Length == 0)
    {
        return nint.Zero;
    }

    var pointer = Marshal.AllocHGlobal(bytes.Length);
    Marshal.Copy(bytes, 0, pointer, bytes.Length);
    return pointer;
}

static string CopyUtf8(AfUtf8BufferNative buffer)
{
    Require(
        buffer.Data != nint.Zero &&
        buffer.Length > 0 &&
        buffer.Owner != 0 &&
        buffer.Capacity >= buffer.Length &&
        buffer.Length <= (nuint)int.MaxValue,
        "Raw result metadata is invalid.");

    var bytes = new byte[checked((int)buffer.Length)];
    Marshal.Copy(buffer.Data, bytes, 0, bytes.Length);
    return new UTF8Encoding(false, true).GetString(bytes);
}

static float[] CopyF32(AfF32BufferNative buffer)
{
    Require(
        buffer.Data != nint.Zero &&
        buffer.Length > 0 &&
        buffer.Owner != 0 &&
        buffer.Capacity >= buffer.Length &&
        buffer.Length <= (nuint)int.MaxValue,
        "Raw f32 result metadata is invalid.");

    var values = new float[checked((int)buffer.Length)];
    Marshal.Copy(buffer.Data, values, 0, values.Length);
    return values;
}

static void RequireFloatBits(float[] actual, float[] expected, string message)
{
    Require(actual.Length == expected.Length, message);
    for (var index = 0; index < actual.Length; index++)
    {
        Require(
            BitConverter.SingleToInt32Bits(actual[index]) ==
            BitConverter.SingleToInt32Bits(expected[index]),
            message);
    }
}

static void ReleaseIfOwned(ref AfUtf8BufferNative buffer)
{
    if (buffer.Owner == 0)
    {
        return;
    }

    var status = AfNative.Utf8BufferFree(ref buffer);
    Require(status == AfStatus.Ok, $"Cleanup free returned {(uint)status}.");
}

static void ReleaseF32IfOwned(ref AfF32BufferNative buffer)
{
    if (buffer.Owner == 0)
    {
        return;
    }

    var status = AfNative.F32BufferFree(ref buffer);
    Require(status == AfStatus.Ok, $"Cleanup f32 free returned {(uint)status}.");
}

static void ReleaseByteIfOwned(ref AfByteBufferNative buffer)
{
    if (buffer.Owner == 0)
    {
        return;
    }

    var status = AfNative.ByteBufferFree(ref buffer);
    Require(status == AfStatus.Ok, $"Cleanup byte free returned {(uint)status}.");
}

static void RequireEmpty(AfUtf8BufferNative buffer, string message)
{
    Require(
        buffer.Data == nint.Zero &&
        buffer.Length == 0 &&
        buffer.Capacity == 0 &&
        buffer.Owner == 0,
        message);
}

static void RequireF32Empty(AfF32BufferNative buffer, string message)
{
    Require(
        buffer.Data == nint.Zero &&
        buffer.Length == 0 &&
        buffer.Capacity == 0 &&
        buffer.Owner == 0,
        message);
}

static void RequireByteEmpty(AfByteBufferNative buffer, string message)
{
    Require(
        buffer.Data == nint.Zero &&
        buffer.Length == 0 &&
        buffer.Capacity == 0 &&
        buffer.Owner == 0,
        message);
}

static bool BuffersEqual(AfUtf8BufferNative left, AfUtf8BufferNative right) =>
    left.Data == right.Data &&
    left.Length == right.Length &&
    left.Capacity == right.Capacity &&
    left.Owner == right.Owner;

static bool F32BuffersEqual(AfF32BufferNative left, AfF32BufferNative right) =>
    left.Data == right.Data &&
    left.Length == right.Length &&
    left.Capacity == right.Capacity &&
    left.Owner == right.Owner;

static bool ByteBuffersEqual(AfByteBufferNative left, AfByteBufferNative right) =>
    left.Data == right.Data &&
    left.Length == right.Length &&
    left.Capacity == right.Capacity &&
    left.Owner == right.Owner;

static string FindModulePath(Process process, string moduleName)
{
    foreach (ProcessModule module in process.Modules)
    {
        if (string.Equals(module.ModuleName, moduleName, StringComparison.OrdinalIgnoreCase))
        {
            return Path.GetFullPath(module.FileName);
        }
    }

    throw new InvalidOperationException($"Loaded module not found: {moduleName}");
}

static bool PathsEqual(string left, string right) =>
    string.Equals(Path.GetFullPath(left), Path.GetFullPath(right), StringComparison.OrdinalIgnoreCase);

static TException ExpectThrows<TException>(Action action, string message)
    where TException : Exception
{
    try
    {
        action();
    }
    catch (TException exception)
    {
        return exception;
    }

    throw new InvalidOperationException(message);
}

static void Require(bool condition, string message)
{
    if (!condition)
    {
        throw new InvalidOperationException(message);
    }
}
