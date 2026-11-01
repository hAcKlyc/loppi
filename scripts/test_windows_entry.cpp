// Real Win32 geometry/region checks; no desktop pixels or user content read.
#include "../src-tauri/native/windows.cpp"
#include <cassert>
static int completions=0;
static uint64_t completion_generation=0;
static void observed(int kind,int,uint64_t generation){
  if(kind==8){completions++;completion_generation=generation;}
}
int main(){
  SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
  HWND window=CreateWindowExW(WS_EX_TOOLWINDOW,L"STATIC",L"loppi synthetic geometry",WS_POPUP,0,0,68,68,nullptr,nullptr,GetModuleHandleW(nullptr),nullptr);
  assert(window);loppi_entry_install(window,observed);
  const double anchors[][2]={{2500,200},{-5000,-5000},{100000,100000},{1e200,-1e200},{0,0}};
  for(auto& anchor:anchors){
    loppi_entry_restore(window,"",anchor[0],anchor[1]);
    for(int view:{0,1,2}){
      Geometry geometry=loppi_entry_layout(window,1,view,0,0,1,view+1);
      assert(geometry.rabbit_x>=0&&geometry.rabbit_y>=0);
      assert(geometry.rabbit_x+52<=geometry.width&&geometry.rabbit_y+52<=geometry.height);
      HRGN region=CreateRectRgn(0,0,0,0);assert(GetWindowRgn(window,region)!=ERROR);
      double dpi=scale(window);assert(PtInRegion(region,int((geometry.rabbit_x+26)*dpi),int((geometry.rabbit_y+26)*dpi)));
      DeleteObject(region);
    }
  }
  for(int view:{1,2}){
    for(int detail:{0,1}){
      int before=completions;
      uint64_t generation=uint64_t(10+view*2+detail);
      Geometry geometry=loppi_entry_layout(window,0,view,0,detail,1,generation);
      assert(geometry.width==412 && geometry.height==168+(detail && view==1 ? 40 : 0));
      assert(geometry.band==24 && geometry.top_band==0);
      assert(geometry.rabbit_x==16 && geometry.rabbit_y==38);
      assert(geometry.rabbit_x+96<=geometry.width && geometry.rabbit_y+96<=geometry.height);
      assert(completions==before+(view==1 ? 1 : 0));
      if(view==1)assert(completion_generation==generation);
      HRGN region=CreateRectRgn(0,0,0,0);assert(GetWindowRgn(window,region)!=ERROR);
      double dpi=scale(window);
      assert(PtInRegion(region,int(206*dpi),int(102*dpi))); // persistent comment row
      assert(!PtInRegion(region,0,int((geometry.height-1)*dpi)));
      DeleteObject(region);
    }
  }
  loppi_entry_dispose(window);DestroyWindow(window);return 0;
}
