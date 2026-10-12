using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization.Metadata;
using PgnSimpleEdit.Models;

namespace PgnSimpleEdit.Interop;

/// <summary>Receives events from a running core operation. Called on a worker thread.</summary>
internal interface ICoreListener
{
    /// <param name="total">Zero when the amount of work is unknown.</param>
    void OnProgress(ulong done, ulong total, ulong games);
    void OnPhase(string text);
    void OnLog(string text);
}

/// <summary>An operation failed in the core; the message is written for the user.</summary>
internal sealed class CoreException(string message) : Exception(message);

/// <summary>Runs requests against the native core.</summary>
internal static unsafe class CoreClient
{
    private sealed record RunState(ICoreListener? Listener, CancellationToken Cancellation);

    // Checked once; a failure is remembered and thrown again on every use.
    private static readonly Lazy<string> Version = new(LoadVersion);

    /// <summary>The core's version, after checking that it speaks the ABI this app was built for.</summary>
    /// <exception cref="CoreException">The core is a different version than this app needs.</exception>
    public static string GetVersion() => Version.Value;

    private static string LoadVersion()
    {
        uint abi = Native.tt_abi_version();
        if (abi != Native.AbiVersion)
            throw new CoreException($"titania_core.dll has ABI version {abi}, but this app needs version {Native.AbiVersion}. Reinstall the app.");
        return Marshal.PtrToStringUTF8((nint)Native.tt_version()) ?? "";
    }

    /// <summary>Runs a request on a worker thread.</summary>
    /// <exception cref="OperationCanceledException">The token was cancelled.</exception>
    /// <exception cref="CoreException">The operation failed.</exception>
    public static Task<TResult> RunAsync<TResult>(CoreRequest request, JsonTypeInfo<TResult> resultType, ICoreListener? listener, CancellationToken cancellation) =>
        Task.Run(() => Run(request, resultType, listener, cancellation), CancellationToken.None);

    private static TResult Run<TResult>(CoreRequest request, JsonTypeInfo<TResult> resultType, ICoreListener? listener, CancellationToken cancellation)
    {
        // Nothing is asked of a core that does not speak the ABI this app was built for.
        _ = Version.Value;
        byte[] json = JsonSerializer.SerializeToUtf8Bytes(request, CoreJson.Default.CoreRequest);
        GCHandle handle = GCHandle.Alloc(new RunState(listener, cancellation));
        byte* output = null;
        nuint outputLen = 0;
        try
        {
            int status;
            fixed (byte* p = json)
                status = Native.tt_run(p, (nuint)json.Length, &OnEvent, (void*)GCHandle.ToIntPtr(handle), &output, &outputLen);

            var document = new ReadOnlySpan<byte>(output, checked((int)outputLen));
            if (status == Native.Ok)
                return JsonSerializer.Deserialize(document, resultType) ?? throw new CoreException("The core returned an empty result.");
            if (status == Native.Cancelled)
                throw new OperationCanceledException(cancellation);
            throw new CoreException(JsonSerializer.Deserialize(document, CoreJson.Default.CoreError)?.Error ?? "The operation failed.");
        }
        finally
        {
            if (output != null) Native.tt_free(output, outputLen);
            handle.Free();
        }
    }

    // Called by the core on the thread that called tt_run. Returns non-zero to cancel.
    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    private static int OnEvent(void* context, int kind, ulong done, ulong total, ulong games, byte* text, nuint textLen)
    {
        try
        {
            var state = (RunState)GCHandle.FromIntPtr((nint)context).Target!;
            if (state.Listener is { } listener)
            {
                switch (kind)
                {
                    case Native.EventProgress: listener.OnProgress(done, total, games); break;
                    case Native.EventPhase: listener.OnPhase(Encoding.UTF8.GetString(text, (int)textLen)); break;
                    case Native.EventLog: listener.OnLog(Encoding.UTF8.GetString(text, (int)textLen)); break;
                }
            }
            return state.Cancellation.IsCancellationRequested ? 1 : 0;
        }
        catch
        {
            // An exception must not unwind into native code; stop the operation instead.
            return 1;
        }
    }
}
