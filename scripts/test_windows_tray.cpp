// Exercise the real subclass message path; stub only the modal menu choice.
// Physical taskbar mouse/keyboard interaction remains separate OS acceptance.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <cassert>
static int popup_count = 0, selected = 1, observed_kind = 0;
static POINT popup_anchor{};
static BOOL WINAPI observe_popup(HMENU menu, UINT flags, int x, int y, int,
                                 HWND owner, const RECT *) {
  assert(IsWindow(owner) && GetMenuItemCount(menu) == 2);
  assert(flags & TPM_RETURNCMD);
  popup_count++;
  popup_anchor = {x, y};
  return selected;
}
#define TrackPopupMenu observe_popup
#include "../src-tauri/native/windows.cpp"
#undef TrackPopupMenu
static void observed(int kind, int, uint64_t) { observed_kind = kind; }
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
  assert(popup_count == 2 && observed_kind == 12);
  assert(popup_anchor.x == 600 && popup_anchor.y == 700);
  selected = 1;
  SendMessageW(window, WM_APP + 42, 42, MAKELPARAM(WM_RBUTTONUP, 42));
  assert(popup_count == 3 && observed_kind == 11);
  loppi_entry_dispose(window);
  DestroyWindow(window);
}
