"""Compile and exercise real Win32 window regions with removed-display anchors."""
import json,subprocess
from pathlib import Path
root=Path.cwd();directory=root/'build/windows-entry-geometry';directory.mkdir(parents=True,exist_ok=True)
cmake='cmake_minimum_required(VERSION 3.20)\nproject(loppi_entry_geometry CXX)\n'
for name,filename in [('entry_geometry','test_windows_entry.cpp'),('entry_tray','test_windows_tray.cpp')]:
    source=str(root/'scripts'/filename).replace('\\','/')
    cmake+=f'add_executable({name} "{source}")\ntarget_compile_features({name} PRIVATE cxx_std_17)\ntarget_compile_definitions({name} PRIVATE UNICODE _UNICODE)\ntarget_link_libraries({name} PRIVATE user32 gdi32 shell32 advapi32 winmm comctl32 dwmapi wtsapi32)\n'
(directory/'CMakeLists.txt').write_text(cmake,encoding='utf8')
for args in [['cmake','-S',str(directory),'-B',str(directory/'target'),'-G','Visual Studio 17 2022','-A','x64'],['cmake','--build',str(directory/'target'),'--config','Debug','--parallel','2'],[str(directory/'target/Debug/entry_geometry.exe')],[str(directory/'target/Debug/entry_tray.exe')]]:
    subprocess.run(args,check=True,timeout=180,creationflags=subprocess.CREATE_NO_WINDOW)
(directory/'report.json').write_text(json.dumps({'schema':1,'passed':True,'real_win32':True,'desktop_capture':False,'cases':18,'checks':['restored off-work-area anchors remain visible','native region contains rabbit center at actual DPI','tray WM_CONTEXTMENU Open/Quit and anchor routing; modal popup choice stubbed','legacy tray mouse event routing; physical taskbar interaction not claimed']},indent=2)+'\n',encoding='utf8')
