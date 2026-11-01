#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h> // Must precede Windows SDK dependent headers.
#include <algorithm>
#include <cmath>
#include <commctrl.h>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <dwmapi.h>
#include <mmsystem.h>
#include <shellapi.h>
#include <string>
#include <windowsx.h>
#include <wtsapi32.h>
using Callback = void (*)(int, int, uint64_t);
struct Geometry {
  double width, height, band, rabbit_x, rabbit_y, top_band, top_shift, menu_y;
};
extern "C" Geometry loppi_entry_layout(void *value, int mode, int view,
                                       int feedback, int detail, int suppress,
                                       uint64_t generation);
struct Entry {
  HWND window;
  Callback callback;
  Geometry geometry{};
  int mode = 0, view = 0;
  bool hover = false, drag = false, tracking = false, menu_active = false;
  uint64_t generation = 0;
  double anchor_x = 0, anchor_y = 0;
  HWND prior = nullptr;
  NOTIFYICONDATAW tray{};
};
static Entry *entry(HWND w) {
  return static_cast<Entry *>(GetPropW(w, L"LoppiEntry"));
}
static UINT taskbar_created = RegisterWindowMessageW(L"TaskbarCreated");
static double scale(HWND w) {
  UINT dpi = GetDpiForWindow(w);
  return (dpi ? dpi : 96) / 96.0;
}
static void notify(Entry *e, int kind, int value) {
  if (e && e->callback)
    e->callback(kind, value, e->generation);
}
static bool rounded(double x, double y, double width, double height,
                    double radius) {
  if (x < 0 || y < 0 || x >= width || y >= height)
    return false;
  double cx = std::clamp(x, radius, width - radius),
         cy = std::clamp(y, radius, height - radius);
  return (x - cx) * (x - cx) + (y - cy) * (y - cy) <= radius * radius;
}
static bool hit(Entry *e, POINT p) {
  RECT r;
  GetWindowRect(e->window, &r);
  double dpi = scale(e->window), x = (p.x - r.left) / dpi,
         y = (p.y - r.top) / dpi;
  auto g = e->geometry;
  if (e->mode == 0)
    return e->view != 0 && rounded(x, y, g.width, g.height, 26);
  double cx = g.rabbit_x + 26, cy = g.rabbit_y + 26;
  bool rabbit = (x - cx) * (x - cx) + (y - cy) * (y - cy) <= 31 * 31;
  return rabbit || (e->view != 0 && rounded(x - 8, y - g.menu_y, 116, 167, 20));
}
static LRESULT CALLBACK procedure(HWND w, UINT msg, WPARAM wp, LPARAM lp,
                                  UINT_PTR, DWORD_PTR) {
  Entry *e = entry(w);
  if (!e)
    return DefSubclassProc(w, msg, wp, lp);
  if (msg == WM_APP + 43) {
    EndMenu();
    notify(e, 12, 1);
    return 0;
  }
  if (msg == taskbar_created) {
    if (Shell_NotifyIconW(NIM_ADD, &e->tray)) {
      e->tray.uVersion = NOTIFYICON_VERSION_4;
      Shell_NotifyIconW(NIM_SETVERSION, &e->tray);
    }
    return 0;
  }
  if (msg == WM_APP + 42) {
    UINT event = LOWORD(lp);
    if (event == WM_LBUTTONUP || event == NIN_SELECT ||
        event == NIN_KEYSELECT) {
      ShowWindow(w, SW_SHOWNOACTIVATE);
      notify(e, 1, 1);
      return 0;
    }
    if (event == WM_CONTEXTMENU || event == WM_RBUTTONUP) {
      if (e->menu_active)
        return 0;
      e->menu_active = true;
      HMENU menu = CreatePopupMenu();
      AppendMenuW(menu, MF_STRING, 1, L"Open loppi");
      AppendMenuW(menu, MF_STRING, 2, L"Quit loppi");
      // Version 4 supplies the tray anchor here, including keyboard activation.
      POINT p = {GET_X_LPARAM(wp), GET_Y_LPARAM(wp)};
      if (event == WM_RBUTTONUP)
        GetCursorPos(&p);
      SetForegroundWindow(w);
      int chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY, p.x, p.y,
                                  0, w, nullptr);
      DestroyMenu(menu);
      e->menu_active = false;
      PostMessageW(w, WM_NULL, 0, 0);
      if (chosen == 1)
        notify(e, 11, 1);
      if (chosen == 2) {
        EndMenu();
        PostMessageW(w, WM_APP + 43, 0, 0);
      }
      return 0;
    }
  }
  if (msg == WM_TIMER && wp == 42) {
    POINT p;
    GetCursorPos(&p);
    bool inside = IsWindowVisible(w) && hit(e, p);
    if (inside != e->hover && !e->drag) {
      e->hover = inside;
      notify(e, 1, inside ? 1 : 0);
      if (!inside)
        notify(e, 7, 0);
    }
    return 0;
  }
  if (msg == WM_NCHITTEST) {
    POINT p = {GET_X_LPARAM(lp), GET_Y_LPARAM(lp)};
    if (!hit(e, p))
      return HTTRANSPARENT;
    RECT r;
    GetWindowRect(w, &r);
    double dpi = scale(w);
    double x = (p.x - r.left) / dpi - e->geometry.rabbit_x - 26,
           y = (p.y - r.top) / dpi - e->geometry.rabbit_y - 26;
    if (e->mode == 1 && x * x + y * y <= 31 * 31)
      return HTCAPTION;
    return HTCLIENT;
  }
  if (msg == WM_NCLBUTTONDBLCLK)
    return 0;
  if (msg == WM_NCLBUTTONDOWN && wp == HTCAPTION) {
    POINT before, after;
    GetCursorPos(&before);
    // A WebView collapse can update geometry while the OS move loop pumps
    // messages. Preserve the grabbed rabbit's offset for the release anchor.
    const auto drag_geometry = e->geometry;
    const double drag_scale = scale(w);
    e->drag = true;
    notify(e, 5, 1);
    LRESULT result = DefSubclassProc(w, msg, wp, lp);
    GetCursorPos(&after);
    e->drag = false;
    RECT r;
    GetWindowRect(w, &r);
    e->anchor_x = r.left + (drag_geometry.rabbit_x + 26) * drag_scale;
    e->anchor_y = r.top + (drag_geometry.rabbit_y + 26) * drag_scale;
    // Reconcile the HWND with the collapsed geometry before publishing and
    // saving release; the OS move loop may have retained the expanded offset.
    loppi_entry_layout(w, e->mode, 0, 0, 0, 0, e->generation);
    notify(e, 5, 0);
    if (std::abs(after.x - before.x) + std::abs(after.y - before.y) < 4)
      notify(e, 10, 1);
    return result;
  }
  if (msg == WM_DISPLAYCHANGE || msg == WM_DPICHANGED) {
    notify(e, 3, 1);
  }
  if (msg == WM_ACTIVATE && LOWORD(wp) == WA_INACTIVE && e->view == 2)
    notify(e, 2, 1);
  return DefSubclassProc(w, msg, wp, lp);
}
extern "C" int loppi_entry_active() {
  DWORD pid = 0;
  GetWindowThreadProcessId(GetForegroundWindow(), &pid);
  return pid == GetCurrentProcessId();
}
extern "C" void loppi_entry_install(void *value, Callback callback) {
  HWND w = static_cast<HWND>(value);
  auto e = new Entry();
  e->window = w;
  e->callback = callback;
  SetPropW(w, L"LoppiEntry", e);
  SetWindowSubclass(w, procedure, 42, 0);
  SetTimer(w, 42, 40, nullptr);
  e->tray.cbSize = sizeof(e->tray);
  e->tray.hWnd = w;
  e->tray.uID = 42;
  e->tray.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
  e->tray.uCallbackMessage = WM_APP + 42;
  wchar_t path[32768];
  DWORD n = GetModuleFileNameW(nullptr, path, 32768);
  e->tray.hIcon =
      n ? ExtractIconW(GetModuleHandleW(nullptr), path, 0) : nullptr;
  if (!e->tray.hIcon)
    e->tray.hIcon = LoadIconW(nullptr, IDI_APPLICATION);
  wcscpy_s(e->tray.szTip, L"loppi");
  if (!Shell_NotifyIconW(NIM_ADD, &e->tray)) {
    notify(e, 6, 0);
  } else {
    e->tray.uVersion = NOTIFYICON_VERSION_4;
    Shell_NotifyIconW(NIM_SETVERSION, &e->tray);
  }
}
extern "C" Geometry loppi_entry_layout(void *value, int mode, int view,
                                       int feedback, int detail, int suppress,
                                       uint64_t generation) {
  HWND w = static_cast<HWND>(value);
  Entry *e = entry(w);
  if (!e)
    return {};
  e->mode = mode;
  e->view = view;
  e->generation = generation;
  double dpi = scale(w);
  POINT cursor;
  GetCursorPos(&cursor);
  if (mode == 1 && std::isfinite(e->anchor_x) && std::isfinite(e->anchor_y) &&
      (e->anchor_x != 0 || e->anchor_y != 0)) {
    cursor.x =
        (LONG)std::clamp(e->anchor_x, double(LONG_MIN), double(LONG_MAX));
    cursor.y =
        (LONG)std::clamp(e->anchor_y, double(LONG_MIN), double(LONG_MAX));
  }
  MONITORINFO monitor = {sizeof(monitor)};
  GetMonitorInfoW(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST), &monitor);
  RECT work = monitor.rcWork;
  Geometry g = {};
  int x = 0, y = 0;
  if (mode == 0) {
    g.width = view ? 412 : (feedback ? 280 : 44);
    g.height = view ? (view == 2 ? 152 : 128) + (detail && view == 1 ? 40 : 0)
                    : (feedback ? 80 : 32);
    g.rabbit_x = view ? 20 : g.width / 2 - 14;
    g.rabbit_y = view ? 24 : 2;
    g.top_shift = 0;
    RECT tray = {};
    NOTIFYICONIDENTIFIER id = {sizeof(id), w, 42, {}};
    if (SUCCEEDED(Shell_NotifyIconGetRect(&id, &tray))) {
      x = (tray.left + tray.right) / 2 - int(g.width * dpi / 2);
      y = tray.top - int(g.height * dpi) - 8;
    } else {
      x = work.right - int(g.width * dpi) - 12;
      y = work.bottom - int(g.height * dpi) - 12;
    }
    x = std::clamp(
        x, (int)work.left,
        std::max((int)work.left, (int)work.right - int(g.width * dpi)));
    y = std::clamp(
        y, (int)work.top,
        std::max((int)work.top, (int)work.bottom - int(g.height * dpi)));
  } else {
    if (e->anchor_x == 0 && e->anchor_y == 0) {
      e->anchor_x = work.right - 48 * dpi;
      e->anchor_y = work.top + 100 * dpi;
    }
    // Recover saved/dragged anchors after a monitor disconnect or work-area
    // change. Clamp the actual rabbit center before deriving its HWND-local
    // region.
    e->anchor_x = std::clamp(
        e->anchor_x, double(work.left) + 34 * dpi,
        std::max(double(work.left) + 34 * dpi, double(work.right) - 34 * dpi));
    e->anchor_y = std::clamp(
        e->anchor_y, double(work.top) + 34 * dpi,
        std::max(double(work.top) + 34 * dpi, double(work.bottom) - 34 * dpi));
    g.width = view ? 132 : 68;
    g.height = view ? 230 : 68;
    bool right = e->anchor_x > (work.left + work.right) / 2;
    bool down = e->anchor_y + 196 * dpi > work.bottom;
    x = int(e->anchor_x - (view && right ? 98 : 34) * dpi);
    y = int(e->anchor_y - (view && down ? 196 : 34) * dpi);
    x = std::clamp(
        x, (int)work.left,
        std::max((int)work.left, (int)work.right - int(g.width * dpi)));
    y = std::clamp(
        y, (int)work.top,
        std::max((int)work.top, (int)work.bottom - int(g.height * dpi)));
    g.rabbit_x = (e->anchor_x - x) / dpi - 26;
    g.rabbit_y = (e->anchor_y - y) / dpi - 26;
    g.menu_y = down ? 55 : 8;
  }
  e->geometry = g;
  SetWindowPos(w, HWND_TOPMOST, x, y, int(std::round(g.width * dpi)),
               int(std::round(g.height * dpi)), SWP_NOACTIVATE);
  // Region and hit testing share the exact CSS radii and DPI transform.
  HRGN region = mode == 0 ? CreateRoundRectRgn(0, 0, int(g.width * dpi) + 1,
                                               int(g.height * dpi) + 1,
                                               int(52 * dpi), int(52 * dpi))
                          : CreateEllipticRgn(int(g.rabbit_x * dpi),
                                              int(g.rabbit_y * dpi),
                                              int((g.rabbit_x + 52) * dpi),
                                              int((g.rabbit_y + 52) * dpi));
  if (mode == 1 && view) {
    HRGN menu = CreateRoundRectRgn(int(8 * dpi), int(g.menu_y * dpi),
                                   int(124 * dpi), int((g.menu_y + 167) * dpi),
                                   int(40 * dpi), int(40 * dpi));
    CombineRgn(region, region, menu, RGN_OR);
    DeleteObject(menu);
  }
  if (!SetWindowRgn(w, region, TRUE))
    DeleteObject(region);
  if (mode == 0 && view == 0 && !feedback) {
    ShowWindow(w, SW_HIDE);
    notify(e, 6, 0);
  } else if (!suppress) {
    ShowWindow(w, SW_SHOWNOACTIVATE);
    notify(e, 6, 1);
  }
  if (mode == 0 && view == 1)
    notify(e, 8, 1);
  return g;
}
extern "C" Geometry loppi_entry_geometry(void *value) {
  auto e = entry(static_cast<HWND>(value));
  return e ? e->geometry : Geometry{};
}
extern "C" int loppi_entry_focus(void *value, int acquire, int restore,
                                 uint64_t generation) {
  auto e = entry(static_cast<HWND>(value));
  if (!e)
    return 0;
  if (generation < e->generation)
    return 0;
  if (acquire) {
    e->prior = GetForegroundWindow();
    LONG_PTR style = GetWindowLongPtrW(e->window, GWL_EXSTYLE);
    SetWindowLongPtrW(e->window, GWL_EXSTYLE, style & ~WS_EX_NOACTIVATE);
    ShowWindow(e->window, SW_SHOW);
    SetForegroundWindow(e->window);
    SetFocus(e->window);
    bool ok = GetForegroundWindow() == e->window;
    notify(e, 4, ok ? 1 : 0);
    return ok;
  }
  if (restore && IsWindow(e->prior))
    SetForegroundWindow(e->prior);
  notify(e, 4, 0);
  return 1;
}
extern "C" char *loppi_entry_position(void *value) {
  auto e = entry(static_cast<HWND>(value));
  if (!e)
    return nullptr;
  char buffer[256];
  snprintf(buffer, sizeof(buffer),
           "{\"display_uuid\":\"windows-desktop\",\"x\":%.3f,\"y\":%.3f}",
           e->anchor_x, e->anchor_y);
  return _strdup(buffer);
}
extern "C" void loppi_entry_restore(void *value, const char *, double x,
                                    double y) {
  auto e = entry(static_cast<HWND>(value));
  if (e && std::isfinite(x) && std::isfinite(y)) {
    e->anchor_x = x;
    e->anchor_y = y;
  }
}
extern "C" uint32_t loppi_entry_window_id(void *value) {
  return (uint32_t)HandleToLong(value);
}
extern "C" void loppi_entry_dispose(void *value) {
  HWND w = static_cast<HWND>(value);
  auto e = entry(w);
  if (!e)
    return;
  KillTimer(w, 42);
  Shell_NotifyIconW(NIM_DELETE, &e->tray);
  if (e->tray.hIcon)
    DestroyIcon(e->tray.hIcon);
  RemoveWindowSubclass(w, procedure, 42);
  RemovePropW(w, L"LoppiEntry");
  delete e;
}

static const wchar_t *run_key =
    L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";
static std::wstring login_command() {
  wchar_t path[32768];
  DWORD n = GetModuleFileNameW(nullptr, path, 32768);
  if (!n)
    return {};
  return L"\"" + std::wstring(path, n) + L"\" --login";
}
extern "C" int loppi_agent_status() {
  wchar_t buffer[32768];
  DWORD size = sizeof(buffer);
  LSTATUS error = RegGetValueW(HKEY_CURRENT_USER, run_key, L"loppi",
                               RRF_RT_REG_SZ, nullptr, buffer, &size);
  if (error == ERROR_FILE_NOT_FOUND)
    return 0;
  if (error)
    return -int(error);
  if (login_command() != buffer)
    return 0;
  BYTE approved[32] = {};
  size = sizeof(approved);
  error = RegGetValueW(HKEY_CURRENT_USER,
                       L"Software\\Microsoft\\Windows\\CurrentVersion\\Explorer"
                       L"\\StartupApproved\\Run",
                       L"loppi", RRF_RT_REG_BINARY, nullptr, approved, &size);
  if (error == ERROR_SUCCESS && size > 0 &&
      (approved[0] == 3 || approved[0] == 7))
    return 2;
  return 1;
}
extern "C" int loppi_agent_register() {
  HKEY key;
  LSTATUS error = RegCreateKeyExW(HKEY_CURRENT_USER, run_key, 0, nullptr, 0,
                                  KEY_SET_VALUE, nullptr, &key, nullptr);
  if (error)
    return int(error);
  auto command = login_command();
  error = command.empty()
              ? ERROR_INVALID_NAME
              : RegSetValueExW(key, L"loppi", 0, REG_SZ,
                               reinterpret_cast<const BYTE *>(command.c_str()),
                               DWORD((command.size() + 1) * sizeof(wchar_t)));
  RegCloseKey(key);
  return int(error);
}
extern "C" int loppi_agent_unregister() {
  HKEY key;
  LSTATUS error = RegOpenKeyExW(HKEY_CURRENT_USER, run_key, 0,
                                KEY_QUERY_VALUE | KEY_SET_VALUE, &key);
  if (error == ERROR_FILE_NOT_FOUND)
    return 0;
  if (error)
    return int(error);
  wchar_t buffer[32768] = {};
  DWORD size = sizeof(buffer), type = 0;
  error = RegQueryValueExW(key, L"loppi", nullptr, &type,
                           reinterpret_cast<BYTE *>(buffer), &size);
  if (error == ERROR_SUCCESS && type == REG_SZ && size <= sizeof(buffer) &&
      size >= sizeof(wchar_t) && buffer[size / sizeof(wchar_t) - 1] == 0 &&
      login_command() == buffer)
    error = RegDeleteValueW(key, L"loppi");
  RegCloseKey(key);
  return error == ERROR_FILE_NOT_FOUND ? 0 : int(error);
}
extern "C" int loppi_microphone_authorization_status() {
  wchar_t value[32];
  DWORD size = sizeof(value);
  const wchar_t *path = L"Software\\Microsoft\\Windows\\CurrentVersion\\Capabil"
                        L"ityAccessManager\\ConsentStore\\microphone";
  for (HKEY key : {HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER}) {
    size = sizeof(value);
    if (RegGetValueW(key, path, L"Value", RRF_RT_REG_SZ, nullptr, value,
                     &size) == ERROR_SUCCESS &&
        !_wcsicmp(value, L"Deny"))
      return 2;
  }
  return 3;
}
extern "C" int loppi_request_microphone_authorization() {
  int status = loppi_microphone_authorization_status();
  if (status != 3)
    ShellExecuteW(nullptr, L"open", L"ms-settings:privacy-microphone", nullptr,
                  nullptr, SW_SHOWNORMAL);
  return status;
}
extern "C" int loppi_microphone_available() { return waveInGetNumDevs() > 0; }
extern "C" int loppi_tap_required() { return 0; }
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
extern "C" int loppi_screen_authorization_status() {
  if (!desktop_unlocked())
    return 0;
  HDESK desktop = OpenInputDesktop(0, FALSE, DESKTOP_READOBJECTS);
  if (!desktop)
    return 0;
  CloseDesktop(desktop);
  return 1;
}
extern "C" int loppi_request_screen_authorization() {
  return loppi_screen_authorization_status();
}
extern "C" void loppi_show_exit_error(const char *message) {
  int length = MultiByteToWideChar(CP_UTF8, 0, message, -1, nullptr, 0);
  std::wstring text(length, 0);
  MultiByteToWideChar(CP_UTF8, 0, message, -1, text.data(), length);
  MessageBoxW(nullptr, text.c_str(), L"loppi", MB_OK | MB_ICONWARNING);
}
extern "C" int loppi_open_url(const char *url) {
  int length = MultiByteToWideChar(CP_UTF8, 0, url, -1, nullptr, 0);
  std::wstring text(length, 0);
  MultiByteToWideChar(CP_UTF8, 0, url, -1, text.data(), length);
  return reinterpret_cast<INT_PTR>(ShellExecuteW(nullptr, L"open", text.c_str(),
                                                 nullptr, nullptr,
                                                 SW_SHOWNORMAL)) > 32;
}
extern "C" int loppi_copy_text(const char *text) {
  int length = MultiByteToWideChar(CP_UTF8, 0, text, -1, nullptr, 0);
  if (!length || !OpenClipboard(nullptr))
    return 0;
  HGLOBAL memory = GlobalAlloc(GMEM_MOVEABLE, length * sizeof(wchar_t));
  if (!memory) {
    CloseClipboard();
    return 0;
  }
  auto buffer = static_cast<wchar_t *>(GlobalLock(memory));
  if (!buffer) {
    GlobalFree(memory);
    CloseClipboard();
    return 0;
  }
  MultiByteToWideChar(CP_UTF8, 0, text, -1, buffer, length);
  GlobalUnlock(memory);
  if (!EmptyClipboard() || !SetClipboardData(CF_UNICODETEXT, memory)) {
    GlobalFree(memory);
    CloseClipboard();
    return 0;
  }
  CloseClipboard();
  return 1;
}
