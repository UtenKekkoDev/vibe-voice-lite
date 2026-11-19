"""Follow the Windows multimedia output without reloading the speech model."""
import ctypes
from uuid import UUID


def default_endpoint_id():
    """Read the live MMDevice endpoint, independent of PortAudio's cached list."""
    ptr = ctypes.c_void_p
    out_ptr = ctypes.POINTER(ptr)
    get_default = ctypes.WINFUNCTYPE(ctypes.HRESULT, ctypes.c_int, ctypes.c_int, out_ptr)(
        4, "GetDefaultAudioEndpoint"
    )
    get_id = ctypes.WINFUNCTYPE(ctypes.HRESULT, out_ptr)(5, "GetId")
    release = ctypes.WINFUNCTYPE(ctypes.c_ulong)(2, "Release")
    ole = ctypes.windll.ole32
    # PortAudio can already have initialized COM in another apartment model.
    hr = ole.CoInitializeEx(None, 0)
    if hr < 0 and hr != ctypes.c_long(0x80010106).value:  # RPC_E_CHANGED_MODE
        raise OSError(f"CoInitializeEx failed ({hr:#x})")
    enumerator, device, device_id = ptr(), ptr(), ptr()
    try:
        clsid = UUID("bcde0395-e52f-467c-8e3d-c4579291692e").bytes_le
        iid = UUID("a95664d2-9614-4f35-a746-de8db63617e6").bytes_le
        ctypes.oledll.ole32.CoCreateInstance(clsid, None, 1, iid, ctypes.byref(enumerator))
        get_default(enumerator, 0, 1, ctypes.byref(device))  # eRender, eMultimedia
        get_id(device, ctypes.byref(device_id))
        return ctypes.wstring_at(device_id)
    finally:
        if device_id:
            ole.CoTaskMemFree(device_id)
        if device:
            release(device)
        if enumerator:
            release(enumerator)
        if hr >= 0:
            ole.CoUninitialize()


class DefaultOutput:
    def __init__(self, sounddevice, callback):
        self.sd = sounddevice
        self.callback = callback
        self.endpoint = None
        self.stream = None

    def ensure(self):
        endpoint = default_endpoint_id()
        if self.stream is not None and self.endpoint == endpoint and self.stream.active:
            return
        self.close()
        # The worker owns all PortAudio streams in this process. Enumerating
        # again with query_hostapis alone would return the previous default.
        if self.sd._initialized:
            self.sd._terminate()
        self.sd._initialize()
        hosts = self.sd.query_hostapis()
        wasapi = next((host for host in hosts if host["name"] == "Windows WASAPI"), None)
        index = wasapi["default_output_device"] if wasapi else self.sd.default.device[1]
        if index is None or index < 0:
            raise RuntimeError("No default audio output")
        stream = self.sd.OutputStream(
            device=index, samplerate=24000, channels=1, dtype="float32",
            blocksize=0, latency="low", callback=self.callback,
            extra_settings=self.sd.WasapiSettings(auto_convert=True) if wasapi else None,
        )
        try:
            stream.start()
        except Exception:
            stream.close()
            raise
        self.stream = stream
        self.endpoint = endpoint

    def close(self):
        stream, self.stream = self.stream, None
        if stream is not None:
            try:
                stream.abort()
            finally:
                stream.close()
