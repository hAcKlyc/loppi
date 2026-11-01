// Windows OS facts and capture only; Rust Core retains policy and media
// ownership.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h> // Must precede Windows SDK dependent headers.
#include <algorithm>
#include <atomic>
#include <audioclient.h>
#include <audioclientactivationparams.h>
#include <cmath>
#include <dwmapi.h>
#include <mmdeviceapi.h>
#include <mutex>
#include <powrprof.h>
#include <psapi.h>
#include <string>
#include <thread>
#include <tlhelp32.h>
#include <vector>
#include <wincodec.h>
#include <wrl.h>
#include <wtsapi32.h>
using Microsoft::WRL::ComPtr;
static std::wstring wide(const char *s) {
  if (!s)
    return {};
  int n = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, s, -1, nullptr, 0);
  if (!n)
    return {};
  std::wstring r(n, 0);
  MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, s, -1, r.data(), n);
  r.pop_back();
  return r;
}
static std::wstring exe_path(DWORD pid) {
  HANDLE p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
  if (!p)
    return {};
  std::wstring s(32768, 0);
  DWORD n = (DWORD)s.size();
  BOOL ok = QueryFullProcessImageNameW(p, 0, s.data(), &n);
  CloseHandle(p);
  if (!ok)
    return {};
  s.resize(n);
  return s;
}
static std::vector<BYTE> process_user(DWORD pid) {
  std::vector<BYTE> data;
  HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid),
         token = nullptr;
  if (!process)
    return data;
  if (OpenProcessToken(process, TOKEN_QUERY, &token)) {
    DWORD size = 0;
    GetTokenInformation(token, TokenUser, nullptr, 0, &size);
    data.resize(size);
    if (!GetTokenInformation(token, TokenUser, data.data(), size, &size))
      data.clear();
    CloseHandle(token);
  }
  CloseHandle(process);
  return data;
}
static DWORD gui_pid() {
  std::wstring own = exe_path(GetCurrentProcessId());
  auto slash = own.find_last_of(L"\\/");
  if (slash == std::wstring::npos)
    return 0;
  std::wstring wanted = own.substr(0, slash + 1) + L"loppi-desktop.exe";
  DWORD session = 0;
  ProcessIdToSessionId(GetCurrentProcessId(), &session);
  HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
  if (snapshot == INVALID_HANDLE_VALUE)
    return 0;
  PROCESSENTRY32W item = {sizeof(item)};
  DWORD found = 0;
  auto own_user = process_user(GetCurrentProcessId());
  for (BOOL ok = Process32FirstW(snapshot, &item); ok;
       ok = Process32NextW(snapshot, &item)) {
    if (_wcsicmp(item.szExeFile, L"loppi-desktop.exe"))
      continue;
    DWORD other = 0;
    if (!ProcessIdToSessionId(item.th32ProcessID, &other) || other != session)
      continue;
    auto other_user = process_user(item.th32ProcessID);
    if (own_user.empty() || other_user.empty() ||
        !EqualSid(reinterpret_cast<TOKEN_USER *>(own_user.data())->User.Sid,
                  reinterpret_cast<TOKEN_USER *>(other_user.data())->User.Sid))
      continue;
    if (!_wcsicmp(exe_path(item.th32ProcessID).c_str(), wanted.c_str())) {
      found = item.th32ProcessID;
      break;
    }
  }
  CloseHandle(snapshot);
  return found;
}
extern "C" uint64_t loppi_playback_exclusion_generation() {
  DWORD pid = gui_pid();
  return pid ? uint64_t(pid) + 1 : 1;
}

class Activation final
    : public Microsoft::WRL::RuntimeClass<
          Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>,
          Microsoft::WRL::FtmBase, IActivateAudioInterfaceCompletionHandler> {
public:
  HANDLE done = CreateEventW(nullptr, TRUE, FALSE, nullptr);
  HRESULT result = E_PENDING;
  ComPtr<IAudioClient> client;
  ~Activation() {
    if (done)
      CloseHandle(done);
  }
  STDMETHOD(ActivateCompleted)(
      IActivateAudioInterfaceAsyncOperation *operation) override {
    ComPtr<IUnknown> value;
    HRESULT activated = E_FAIL;
    result = operation->GetActivateResult(&activated, &value);
    if (SUCCEEDED(result))
      result = activated;
    if (SUCCEEDED(result))
      result = value.As(&client);
    SetEvent(done);
    return S_OK;
  }
};
using AudioCallback = void (*)(void *, const float *, uint32_t, uint32_t,
                               uint32_t, uint64_t);
struct Audio {
  std::thread thread;
  HANDLE stop = CreateEventW(nullptr, TRUE, FALSE, nullptr),
         ready = CreateEventW(nullptr, TRUE, FALSE, nullptr),
         ended = CreateEventW(nullptr, TRUE, FALSE, nullptr);
  std::atomic<int> error{0};
  uint64_t generation = 1;
  void *context;
  AudioCallback callback;
  ~Audio() {
    CloseHandle(stop);
    CloseHandle(ready);
    CloseHandle(ended);
  }
  void run() {
    HRESULT apartment = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    HRESULT hr = apartment;
    if (SUCCEEDED(hr))
      hr = capture();
    if (FAILED(hr))
      error.store((int)hr);
    SetEvent(ready);
    if (SUCCEEDED(apartment))
      CoUninitialize();
    SetEvent(ended);
  }
  HRESULT capture() {
    DWORD pid = gui_pid();
    generation = pid ? uint64_t(pid) + 1 : 1;
    AUDIOCLIENT_ACTIVATION_PARAMS parameters = {};
    parameters.ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK;
    parameters.ProcessLoopbackParams.TargetProcessId =
        pid ? pid : GetCurrentProcessId();
    parameters.ProcessLoopbackParams.ProcessLoopbackMode =
        PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE;
    PROPVARIANT props = {};
    props.vt = VT_BLOB;
    props.blob.cbSize = sizeof(parameters);
    props.blob.pBlobData = reinterpret_cast<BYTE *>(&parameters);
    auto activation = Microsoft::WRL::Make<Activation>();
    if (!activation || !activation->done)
      return E_OUTOFMEMORY;
    ComPtr<IActivateAudioInterfaceAsyncOperation> operation;
    HRESULT hr = ActivateAudioInterfaceAsync(
        VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, __uuidof(IAudioClient), &props,
        activation.Get(), &operation);
    if (FAILED(hr))
      return hr;
    HANDLE activation_events[] = {stop, activation->done};
    DWORD wait = WaitForMultipleObjects(2, activation_events, FALSE, 6000);
    if (wait != WAIT_OBJECT_0 + 1)
      return HRESULT_FROM_WIN32(wait == WAIT_TIMEOUT ? ERROR_TIMEOUT
                                                     : ERROR_CANCELLED);
    if (FAILED(activation->result))
      return activation->result;
    ComPtr<IAudioClient> client = activation->client;
    WAVEFORMATEX format = {
        WAVE_FORMAT_IEEE_FLOAT, 2, 48000, 48000 * 8, 8, 32, 0};
    hr = client->Initialize(AUDCLNT_SHAREMODE_SHARED,
                            AUDCLNT_STREAMFLAGS_LOOPBACK |
                                AUDCLNT_STREAMFLAGS_EVENTCALLBACK |
                                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                            0, 0, &format, nullptr);
    if (FAILED(hr))
      return hr;
    HANDLE samples = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!samples)
      return HRESULT_FROM_WIN32(GetLastError());
    struct EventGuard {
      HANDLE h;
      ~EventGuard() { CloseHandle(h); }
    } guard{samples};
    if (FAILED(hr = client->SetEventHandle(samples)))
      return hr;
    ComPtr<IAudioCaptureClient> input;
    if (FAILED(hr = client->GetService(IID_PPV_ARGS(&input))))
      return hr;
    if (FAILED(hr = client->Start()))
      return hr;
    SetEvent(ready);
    HANDLE events[] = {stop, samples};
    std::vector<float> silence;
    unsigned ticks = 0;
    while (true) {
      DWORD signal = WaitForMultipleObjects(2, events, FALSE, 250);
      if (signal == WAIT_OBJECT_0)
        break;
      if (signal == WAIT_FAILED) {
        hr = HRESULT_FROM_WIN32(GetLastError());
        break;
      }
      if (++ticks % 4 == 0 &&
          loppi_playback_exclusion_generation() != generation) {
        hr = HRESULT_FROM_WIN32(ERROR_DEVICE_REINITIALIZATION_NEEDED);
        break;
      }
      UINT32 available = 0;
      hr = input->GetNextPacketSize(&available);
      if (FAILED(hr))
        break;
      while (available) {
        BYTE *bytes = nullptr;
        UINT32 frames = 0;
        DWORD flags = 0;
        UINT64 device = 0, qpc = 0;
        hr = input->GetBuffer(&bytes, &frames, &flags, &device, &qpc);
        if (FAILED(hr))
          break;
        if (frames > 96000) {
          input->ReleaseBuffer(frames);
          hr = E_UNEXPECTED;
          break;
        }
        const float *data = reinterpret_cast<float *>(bytes);
        if (flags & AUDCLNT_BUFFERFLAGS_SILENT) {
          silence.assign(size_t(frames) * 2, 0);
          data = silence.data();
        }
        if (data && frames)
          callback(context, data, frames, 48000, 2,
                   (flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR) ? 0
                                                                 : qpc * 100);
        hr = input->ReleaseBuffer(frames);
        if (FAILED(hr))
          break;
        hr = input->GetNextPacketSize(&available);
        if (FAILED(hr))
          break;
      }
      if (FAILED(hr))
        break;
    }
    client->Stop();
    return hr;
  }
};
extern "C" int loppi_system_audio_tap_available() {
  return 0;
} // No macOS permission-upgrade mechanism on Windows.
extern "C" void *loppi_system_audio_start(void *context, AudioCallback callback,
                                          int *stage, int, int *native) {
  *stage = 1;
  *native = 0;
  if (!context || !callback)
    return nullptr;
  Audio *audio = new (std::nothrow) Audio();
  if (!audio) {
    *native = E_OUTOFMEMORY;
    return nullptr;
  }
  audio->context = context;
  audio->callback = callback;
  audio->thread = std::thread([audio] { audio->run(); });
  DWORD wait = WaitForSingleObject(audio->ready, 7000);
  if (wait != WAIT_OBJECT_0 || audio->error.load() != 0) {
    SetEvent(audio->stop);
    if (WaitForSingleObject(audio->ended, 2000) != WAIT_OBJECT_0) {
      TerminateProcess(GetCurrentProcess(), 70);
    }
    audio->thread.join();
    *native = audio->error.load() ? audio->error.load()
                                  : HRESULT_FROM_WIN32(ERROR_TIMEOUT);
    delete audio;
    return nullptr;
  }
  *stage = 0;
  return audio;
}
extern "C" int loppi_system_audio_error(void *value) {
  return static_cast<Audio *>(value)->error.load();
}
extern "C" uint64_t loppi_system_audio_exclusion_generation(void *value) {
  return static_cast<Audio *>(value)->generation;
}
extern "C" int loppi_system_audio_stop(void *value) {
  auto audio = static_cast<Audio *>(value);
  SetEvent(audio->stop);
  if (WaitForSingleObject(audio->ended, 2000) != WAIT_OBJECT_0)
    return -2;
  audio->thread.join();
  delete audio;
  return 0;
}
extern "C" int64_t loppi_system_audio_probe(uint32_t seconds, uint64_t *peak) {
  struct Probe {
    std::atomic<uint64_t> frames{0};
    std::atomic<uint64_t> peak{0};
  } probe;
  auto callback = [](void *context, const float *samples, uint32_t frames,
                     uint32_t, uint32_t channels, uint64_t) {
    auto p = static_cast<Probe *>(context);
    p->frames.fetch_add(frames);
    uint64_t maximum = 0;
    for (size_t i = 0; i < size_t(frames) * channels; ++i)
      maximum = std::max(
          maximum, uint64_t(std::min(1.0f, std::abs(samples[i])) * 1000000));
    uint64_t old = p->peak.load();
    while (old < maximum && !p->peak.compare_exchange_weak(old, maximum)) {
    };
  };
  int stage = 0, native = 0;
  void *audio = loppi_system_audio_start(&probe, callback, &stage, 0, &native);
  if (!audio)
    return -std::max(1, stage);
  Sleep(seconds * 1000);
  int error = loppi_system_audio_error(audio);
  int stopped = loppi_system_audio_stop(audio);
  if (stopped == -2)
    TerminateProcess(GetCurrentProcess(), 70);
  *peak = probe.peak.load();
  return error ? -1 : static_cast<int64_t>(probe.frames.load());
}

extern "C" int loppi_microphone_permission_status() {
  // Desktop permission is global/user policy; opening CPAL remains the final
  // device check.
  DWORD type = 0;
  wchar_t value[32] = {};
  DWORD size = sizeof(value);
  const wchar_t *path = L"Software\\Microsoft\\Windows\\CurrentVersion\\Capabil"
                        L"ityAccessManager\\ConsentStore\\microphone";
  for (HKEY root : {HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER}) {
    size = sizeof(value);
    if (RegGetValueW(root, path, L"Value", RRF_RT_REG_SZ, &type, value,
                     &size) == ERROR_SUCCESS &&
        !_wcsicmp(value, L"Deny"))
      return 2;
  }
  return 3;
}
extern "C" int loppi_allow_audio_idle_sleep() {
  return SetThreadExecutionState(ES_CONTINUOUS) ? 0 : (int)GetLastError();
}
struct Resource {
  uint32_t memory_pressure, thermal, low_power, on_battery;
  uint64_t physical_memory;
};
extern "C" void loppi_resource_snapshot(Resource *out) {
  MEMORYSTATUSEX memory = {sizeof(memory)};
  SYSTEM_POWER_STATUS power = {};
  *out = {};
  if (GlobalMemoryStatusEx(&memory)) {
    out->physical_memory = memory.ullTotalPhys;
    out->memory_pressure = memory.dwMemoryLoad >= 95   ? 2
                           : memory.dwMemoryLoad >= 85 ? 1
                                                       : 0;
  }
  if (GetSystemPowerStatus(&power)) {
    out->low_power = power.SystemStatusFlag != 0;
    out->on_battery = power.ACLineStatus == 0;
  }
}
extern "C" uint64_t loppi_process_footprint(int pid) {
  HANDLE p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                         FALSE, (DWORD)pid);
  if (!p)
    return 0;
  PROCESS_MEMORY_COUNTERS_EX memory = {};
  BOOL ok = GetProcessMemoryInfo(
      p, reinterpret_cast<PROCESS_MEMORY_COUNTERS *>(&memory), sizeof(memory));
  CloseHandle(p);
  return ok ? memory.PrivateUsage : 0;
}
extern "C" uint64_t loppi_background_footprint() {
  return loppi_process_footprint((int)GetCurrentProcessId());
}
struct Power {
  uint32_t available, sleeping;
  uint64_t generation;
  int64_t sleep_ms;
};
static std::mutex power_mutex;
static Power power_state = {};
static ULONGLONG slept_at = 0;
static ULONG CALLBACK power_event(void *, ULONG type, void *) {
  std::lock_guard<std::mutex> lock(power_mutex);
  if (type == PBT_APMSUSPEND) {
    power_state.sleeping = 1;
    slept_at = GetTickCount64();
  } else if (type == PBT_APMRESUMEAUTOMATIC || type == PBT_APMRESUMESUSPEND) {
    if (power_state.sleeping) {
      power_state.sleep_ms = GetTickCount64() - slept_at;
      power_state.generation++;
    }
    power_state.sleeping = 0;
  }
  return ERROR_SUCCESS;
}
extern "C" void loppi_power_snapshot(Power *out) {
  static std::once_flag once;
  static HPOWERNOTIFY registration = nullptr;
  std::call_once(once, [] {
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS parameters = {power_event, nullptr};
    power_state.available = PowerRegisterSuspendResumeNotification(
                                DEVICE_NOTIFY_CALLBACK, &parameters,
                                &registration) == ERROR_SUCCESS;
  });
  std::lock_guard<std::mutex> lock(power_mutex);
  *out = power_state;
}

static bool desktop_unlocked() {
  DWORD session = 0, size = 0;
  LPWSTR buffer = nullptr;
  if (!ProcessIdToSessionId(GetCurrentProcessId(), &session) ||
      !WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE, session,
                                   WTSSessionInfoEx, &buffer, &size))
    return false;
  auto info = reinterpret_cast<WTSINFOEXW *>(buffer);
  bool unlocked =
      size >= sizeof(WTSINFOEXW) && info->Level == 1 &&
      info->Data.WTSInfoExLevel1.SessionFlags == WTS_SESSIONSTATE_UNLOCK &&
      info->Data.WTSInfoExLevel1.SessionState == WTSActive;
  WTSFreeMemory(buffer);
  if (!unlocked)
    return false;
  HDESK input = OpenInputDesktop(0, FALSE, DESKTOP_READOBJECTS);
  if (!input)
    return false;
  wchar_t input_name[256] = {}, thread_name[256] = {};
  DWORD needed = 0;
  bool desktop =
      GetUserObjectInformationW(input, UOI_NAME, input_name, sizeof(input_name),
                                &needed) &&
      GetUserObjectInformationW(GetThreadDesktop(GetCurrentThreadId()),
                                UOI_NAME, thread_name, sizeof(thread_name),
                                &needed) &&
      !_wcsicmp(input_name, L"Default") && !_wcsicmp(thread_name, L"Default");
  CloseDesktop(input);
  return desktop;
}
extern "C" int loppi_capture_png(const char *path, const uint32_t *ids,
                                 size_t count, int peer) {
  if (!path || count > 8 || peer <= 0)
    return 0;
  auto previous_dpi =
      SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
  if (!previous_dpi)
    return 0;
  struct RestoreDpi {
    DPI_AWARENESS_CONTEXT previous;
    ~RestoreDpi() { SetThreadDpiAwarenessContext(previous); }
  } restore_dpi{previous_dpi};
  std::vector<HWND> windows;
  for (size_t i = 0; i < count; ++i) {
    HWND hwnd = static_cast<HWND>(LongToHandle((LONG)ids[i]));
    DWORD owner = 0;
    GetWindowThreadProcessId(hwnd, &owner);
    if (!IsWindow(hwnd) || owner != (DWORD)peer)
      return -2;
    windows.push_back(hwnd);
  }
  if (!desktop_unlocked())
    return -4;
  struct Restore {
    std::vector<HWND> hidden;
    ~Restore() {
      for (HWND hwnd : hidden)
        if (IsWindow(hwnd))
          ShowWindowAsync(hwnd, SW_SHOWNOACTIVATE);
    }
  } restore;
  for (HWND hwnd : windows)
    if (IsWindowVisible(hwnd)) {
      if (!ShowWindowAsync(hwnd, SW_HIDE))
        return -3;
      restore.hidden.push_back(hwnd);
      ULONGLONG until = GetTickCount64() + 500;
      while (IsWindowVisible(hwnd) && GetTickCount64() < until)
        Sleep(5);
      if (IsWindowVisible(hwnd))
        return -3;
    }
  DwmFlush();
  int x = GetSystemMetrics(SM_XVIRTUALSCREEN),
      y = GetSystemMetrics(SM_YVIRTUALSCREEN),
      w = GetSystemMetrics(SM_CXVIRTUALSCREEN),
      h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
  if (w <= 0 || h <= 0 || uint64_t(w) * h > 32000000)
    return 0;
  HDC screen = GetDC(nullptr);
  if (!screen)
    return 0;
  HDC dc = CreateCompatibleDC(screen);
  HBITMAP bitmap = CreateCompatibleBitmap(screen, w, h);
  HGDIOBJ previous = SelectObject(dc, bitmap);
  BOOL copied = BitBlt(dc, 0, 0, w, h, screen, x, y, SRCCOPY | CAPTUREBLT);
  SelectObject(dc, previous);
  DeleteDC(dc);
  ReleaseDC(nullptr, screen);
  if (!copied) {
    DeleteObject(bitmap);
    return 0;
  }
  HRESULT apartment = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
  ComPtr<IWICImagingFactory> factory;
  HRESULT hr = CoCreateInstance(CLSID_WICImagingFactory, nullptr,
                                CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory));
  ComPtr<IWICBitmap> image;
  ComPtr<IWICStream> stream;
  ComPtr<IWICBitmapEncoder> encoder;
  ComPtr<IWICBitmapFrameEncode> frame;
  auto destination = wide(path);
  if (SUCCEEDED(hr))
    hr = factory->CreateBitmapFromHBITMAP(bitmap, nullptr, WICBitmapIgnoreAlpha,
                                          &image);
  if (SUCCEEDED(hr))
    hr = factory->CreateStream(&stream);
  if (SUCCEEDED(hr))
    hr = stream->InitializeFromFilename(destination.c_str(), GENERIC_WRITE);
  if (SUCCEEDED(hr))
    hr = factory->CreateEncoder(GUID_ContainerFormatPng, nullptr, &encoder);
  if (SUCCEEDED(hr))
    hr = encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache);
  if (SUCCEEDED(hr))
    hr = encoder->CreateNewFrame(&frame, nullptr);
  if (SUCCEEDED(hr))
    hr = frame->Initialize(nullptr);
  if (SUCCEEDED(hr))
    hr = frame->WriteSource(image.Get(), nullptr);
  if (SUCCEEDED(hr))
    hr = frame->Commit();
  if (SUCCEEDED(hr))
    hr = encoder->Commit();
  frame.Reset();
  encoder.Reset();
  stream.Reset();
  image.Reset();
  factory.Reset();
  DeleteObject(bitmap);
  if (SUCCEEDED(apartment))
    CoUninitialize();
  return SUCCEEDED(hr) ? 1 : 0;
}
