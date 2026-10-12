using System.Runtime.InteropServices;

namespace PgnSimpleEdit.Interop;

// The C ABI of titania_core.dll, as declared in core/include/titania_core.h.
internal static unsafe partial class Native
{
    private const string Dll = "titania_core";

    public const uint AbiVersion = 1;

    public const int Ok = 0;
    public const int Cancelled = 1;

    public const int EventProgress = 0;
    public const int EventPhase = 1;
    public const int EventLog = 2;

    [LibraryImport(Dll)] public static partial uint tt_abi_version();

    [LibraryImport(Dll)] public static partial byte* tt_version();

    [LibraryImport(Dll)]
    public static partial int tt_run(
        byte* request,
        nuint requestLen,
        delegate* unmanaged[Cdecl]<void*, int, ulong, ulong, ulong, byte*, nuint, int> callback,
        void* context,
        byte** output,
        nuint* outputLen);

    [LibraryImport(Dll)] public static partial void tt_free(byte* data, nuint len);
}
