// Exercise the real subclass message path; stub modal choice and OS activation.
// Physical taskbar and actual foreground activation remain OS acceptance.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <cassert>
static int popup_count = 0, selected = 1, observed_kind = 0;
static POINT popup_anchor{};
static HWND foreground_window = nullptr;
static int foreground_requests = 0;
static bool allow_foreground = false;
static HWND WINAPI observe_foreground() { return foreground_window; }
static BOOL WINAPI observe_set_foreground(HWND window) {
  foreground_requests++;
  if (allow_foreground)
    foreground_window = window;
  return allow_foreground;
}
static HWND WINAPI observe_focus(HWND window) { return window; }
static BOOL WINAPI observe_show(HWND, int) { return TRUE; }
static BOOL WINAPI observe_popup(HMENU menu, UINT flags, int x, int y, int,
                                 HWND owner, const RECT *) {
  assert(IsWindow(owner) && GetMenuItemCount(menu) == 2);
  assert(flags & TPM_RETURNCMD);
  popup_count++;
  popup_anchor = {x, y};
  return selected;
}
#define TrackPopupMenu observe_popup
#define GetForegroundWindow observe_foreground
#define SetForegroundWindow observe_set_foreground
#define SetFocus observe_focus
#define ShowWindow observe_show
#include "../src-tauri/native/windows.cpp"
#undef TrackPopupMenu
#undef GetForegroundWindow
#undef SetForegroundWindow
#undef SetFocus
#undef ShowWindow
static int focused = 0, dismissed = 0;
static uint64_t confirmed_generation = 0;
static void observed(int kind, int, uint64_t generation) {
  observed_kind = kind;
  if (kind == 9) { focused++; confirmed_generation = generation; }
  if (kind == 2) dismissed++;
}
static void test_focus(HWND window) {
  HWND prior = CreateWindowExW(WS_EX_TOOLWINDOW, L"STATIC", L"prior focus target",
                              WS_POPUP, 0, 0, 68, 68, nullptr, nullptr,
                              GetModuleHandleW(nullptr), nullptr);
  assert(prior);
  foreground_window = prior;
  allow_foreground = false;
  loppi_entry_layout(window, 0, 1, 0, 0, 1, 10);
  assert(!loppi_entry_focus(window, 1, 0, 11)); // future layout cannot own focus
  assert(!loppi_entry_focus(window, 1, 0, 10));
  assert(entry(window)->comment_focus && entry(window)->prior == prior && focused == 0);
  loppi_entry_layout(window, 0, 1, 1, 1, 1, 11);
  foreground_window = window;
  SendMessageW(window, WM_ACTIVATE, WA_ACTIVE, 0);
  assert(focused == 1 && confirmed_generation == 10);
  int requests = foreground_requests;
  assert(loppi_entry_focus(window, 1, 0, 10)); // original intent survives relayout
  assert(entry(window)->prior == prior && foreground_requests == requests && focused == 1);
  allow_foreground = true;
  assert(loppi_entry_focus(window, 0, 1, 11));
  assert(foreground_window == prior && !entry(window)->prior && !entry(window)->comment_focus);
  SendMessageW(window, WM_ACTIVATE, WA_ACTIVE, 0);
  assert(focused == 1); // late activation cannot confirm a released intent
  loppi_entry_layout(window, 0, 1, 0, 0, 1, 12);
  assert(loppi_entry_focus(window, 1, 0, 12));
  assert(focused == 2 && confirmed_generation == 12 && entry(window)->prior == prior);
  assert(!loppi_entry_focus(window, 0, 1, 11)); // stale release cannot end a newer session
  foreground_window = prior;
  requests = foreground_requests;
  SendMessageW(window, WM_ACTIVATE, WA_INACTIVE, 0);
  assert(dismissed == 1 && !entry(window)->comment_focus && !entry(window)->prior);
  assert(loppi_entry_focus(window, 0, 1, 12) && foreground_requests == requests);
  loppi_entry_layout(window, 1, 1, 0, 0, 1, 13);
  int before = dismissed;
  SendMessageW(window, WM_ACTIVATE, WA_INACTIVE, 0);
  assert(dismissed == before && !loppi_entry_focus(window, 1, 0, 13));
  loppi_entry_layout(window, 0, 0, 0, 0, 1, 14);
  assert(!loppi_entry_focus(window, 1, 0, 14));
  DestroyWindow(prior);
}
int main() {
  HWND window = CreateWindowExW(WS_EX_TOOLWINDOW, L"STATIC", L"loppi tray test",
                                WS_POPUP, 0, 0, 68, 68, nullptr, nullptr,
                                GetModuleHandleW(nullptr), nullptr);
  assert(window);
  loppi_entry_install(window, observed);
  SendMessageW(window, WM_APP + 42, MAKELPARAM(-230, 480),
               MAKELPARAM(WM_CONTEXTMENU, 42));
  assert(popup_count == 1 && observed_kind == 11);
  assert(popup_anchor.x == -230 && popup_anchor.y == 480);
  selected = 2;
  SendMessageW(window, WM_APP + 42, MAKELPARAM(600, 700),
               MAKELPARAM(WM_CONTEXTMENU, 42));
  assert(popup_count == 2 && observed_kind != 12);
  MSG pending{};
  bool dispatched_quit = false;
  while (PeekMessageW(&pending, window, WM_APP + 43, WM_APP + 43, PM_REMOVE)) {
    DispatchMessageW(&pending);
    dispatched_quit = true;
  }
  assert(dispatched_quit && observed_kind == 12);
  assert(popup_anchor.x == 600 && popup_anchor.y == 700);
  selected = 1;
  SendMessageW(window, WM_APP + 42, 42, MAKELPARAM(WM_RBUTTONUP, 42));
  assert(popup_count == 3 && observed_kind == 11);
  // Leaving the rabbit rearms the existing UI suppression; a drag must not.
  entry(window)->hover = true;
  entry(window)->drag = true;
  observed_kind = 0;
  SendMessageW(window, WM_TIMER, 42, 0);
  assert(observed_kind == 0);
  entry(window)->drag = false;
  SendMessageW(window, WM_TIMER, 42, 0);
  assert(!entry(window)->hover && observed_kind == 7);
  test_focus(window);
  loppi_entry_dispose(window);
  DestroyWindow(window);
}
