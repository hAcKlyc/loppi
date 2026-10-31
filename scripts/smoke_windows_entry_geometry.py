"""Compile and exercise real Win32 window regions with removed-display anchors."""
import json,subprocess
from pathlib import Path
root=Path.cwd();directory=root/'build/windows-entry-geometry';directory.mkdir(parents=True,exist_ok=True)
source=str(root/'scripts/test_windows_entry.cpp').replace('\\','/')
(directory/'CMakeLists.txt').write_text('cmake_minimum_required(VERSION 3.20)\nproject(loppi_entry_geometry CXX)\nadd_executable(entry_geometry "'+source+'")\ntarget_compile_features(entry_geometry PRIVATE cxx_std_17)\ntarget_compile_definitions(entry_geometry PRIVATE UNICODE _UNICODE)\ntarget_link_libraries(entry_geometry PRIVATE user32 gdi32 shell32 advapi32 winmm comctl32 dwmapi wtsapi32)\n',encoding='utf8')
for args in [['cmake','-S',str(directory),'-B',str(directory/'target'),'-G','Visual Studio 17 2022','-A','x64'],['cmake','--build',str(directory/'target'),'--config','Debug','--parallel','2'],[str(directory/'target/Debug/entry_geometry.exe')]]:
    subprocess.run(args,check=True,timeout=180,creationflags=subprocess.CREATE_NO_WINDOW)
(directory/'report.json').write_text(json.dumps({'schema':1,'passed':True,'real_win32':True,'desktop_capture':False,'cases':15,'checks':['restored off-work-area anchors remain visible','native region contains rabbit center at actual DPI']},indent=2)+'\n',encoding='utf8')
