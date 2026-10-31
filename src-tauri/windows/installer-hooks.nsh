!include "LogicLib.nsh"
!include "x64.nsh"
!macro LOPPI_STOP_INSTALLED_RUNTIME ID
  IfFileExists "$INSTDIR\loppi.exe" 0 loppi_stopped_${ID}
    ReadRegStr $0 SHCTX "${UNINSTKEY}" "InstallLocation"
    ${If} $0 != '$\"$INSTDIR$\"'
      MessageBox MB_OK|MB_ICONSTOP "This directory contains an unrecognized loppi executable. Choose an empty installation directory."
      Abort
    ${EndIf}
    nsExec::ExecToStack /TIMEOUT=45000 '"$INSTDIR\loppi.exe" prepare-install'
    Pop $0
    Pop $1
    ${If} $0 != 0
      MessageBox MB_OK|MB_ICONSTOP "loppi could not finish normal shutdown. Close loppi and retry. Installation files and recordings have not been removed."
      Abort
    ${EndIf}
  loppi_stopped_${ID}:
!macroend
!macro NSIS_HOOK_PREINSTALL
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "loppi requires Windows 11 x64."
    Abort
  ${EndIf}
  System::Alloc 284
  Pop $0
  ${If} $0 == 0
    MessageBox MB_OK|MB_ICONSTOP "Cannot verify the Windows version."
    Abort
  ${EndIf}
  System::Call '*$0(i 284)'
  System::Call 'ntdll::RtlGetVersion(p r0)i.r1'
  ${If} $1 != 0
    System::Free $0
    MessageBox MB_OK|MB_ICONSTOP "Cannot verify the Windows version."
    Abort
  ${EndIf}
  System::Call '*$0(i.r1,i.r2,i.r3,i.r4)'
  System::Free $0
  ${If} $4 < 22000
    MessageBox MB_OK|MB_ICONSTOP "loppi requires Windows 11 x64 (build 22000 or later)."
    Abort
  ${EndIf}
  !insertmacro LOPPI_STOP_INSTALLED_RUNTIME install
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro LOPPI_STOP_INSTALLED_RUNTIME uninstall
  ; Do not delete the user's recordings, notes, keys or configuration.
  ; Remove the startup registration only when it belongs to this installation.
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "loppi"
  ${If} $0 == '$\"$INSTDIR\loppi-desktop.exe$\" --login'
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "loppi"
  ${EndIf}
!macroend
