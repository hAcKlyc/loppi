!include "LogicLib.nsh"
!include "x64.nsh"
; Tauri 2.12's utils.nsh is included before this file. Replace its forceful
; Restart Manager shutdown with a read-only final file-use check. Our CLI owns
; normal shutdown; an unresolved process must abort installation, never be killed.
!ifmacrodef CheckIfAppIsRunning
  !macroundef CheckIfAppIsRunning
!else
  !error "Tauri NSIS process-check contract changed; review installer integration"
!endif
!macro CheckIfAppIsRunning executablePath productName
  !define LOPPI_RM_ID ${__LINE__}
  IfFileExists "${executablePath}" 0 loppi_rm_done_${LOPPI_RM_ID}
  !insertmacro RestartManager_StartSession $R0
  ${If} $R0 == ""
    Goto loppi_rm_failed_${LOPPI_RM_ID}
  ${EndIf}
  !insertmacro RestartManager_RegisterFile $R0 "${executablePath}"
  ${If} $0 != 0
    !insertmacro RestartManager_EndSession $R0
    Goto loppi_rm_failed_${LOPPI_RM_ID}
  ${EndIf}
  System::Call 'RSTRTMGR::RmGetList(p R0, *i .r1, *i .r2, p 0, *i .r3) i .r4'
  Push $4
  !insertmacro RestartManager_EndSession $R0
  Pop $4
  ${If} $4 != 0
    Goto loppi_rm_failed_${LOPPI_RM_ID}
  ${EndIf}
  Goto loppi_rm_done_${LOPPI_RM_ID}
  loppi_rm_failed_${LOPPI_RM_ID}:
    SetErrorLevel 1
    MessageBox MB_OK|MB_ICONSTOP "${productName} installation files are still in use or cannot be checked. Close loppi and retry." /SD IDOK
    Abort
  loppi_rm_done_${LOPPI_RM_ID}:
  !undef LOPPI_RM_ID
!macroend
!macro LOPPI_STOP_INSTALLED_RUNTIME ID
  IfFileExists "$INSTDIR\loppi.exe" 0 loppi_stopped_${ID}
    ReadRegStr $0 SHCTX "${UNINSTKEY}" "InstallLocation"
    ${If} $0 != '$\"$INSTDIR$\"'
      SetErrorLevel 1
      MessageBox MB_OK|MB_ICONSTOP "This directory contains an unrecognized loppi executable. Choose an empty installation directory." /SD IDOK
      Abort
    ${EndIf}
    nsExec::ExecToStack /TIMEOUT=45000 '"$INSTDIR\loppi.exe" prepare-install'
    Pop $0
    Pop $1
    ${If} $0 != 0
      SetErrorLevel 1
      MessageBox MB_OK|MB_ICONSTOP "loppi could not finish normal shutdown. Close loppi and retry. Installation files and recordings have not been removed." /SD IDOK
      Abort
    ${EndIf}
  loppi_stopped_${ID}:
!macroend
!macro NSIS_HOOK_PREINSTALL
  ${IfNot} ${RunningX64}
    SetErrorLevel 1
    MessageBox MB_OK|MB_ICONSTOP "loppi requires Windows 11 x64." /SD IDOK
    Abort
  ${EndIf}
  System::Alloc 284
  Pop $0
  ${If} $0 == 0
    SetErrorLevel 1
    MessageBox MB_OK|MB_ICONSTOP "Cannot verify the Windows version." /SD IDOK
    Abort
  ${EndIf}
  System::Call '*$0(i 284)'
  System::Call 'ntdll::RtlGetVersion(p r0)i.r1'
  ${If} $1 != 0
    System::Free $0
    SetErrorLevel 1
    MessageBox MB_OK|MB_ICONSTOP "Cannot verify the Windows version." /SD IDOK
    Abort
  ${EndIf}
  System::Call '*$0(i.r1,i.r2,i.r3,i.r4)'
  System::Free $0
  ${If} $4 < 22000
    SetErrorLevel 1
    MessageBox MB_OK|MB_ICONSTOP "loppi requires Windows 11 x64 (build 22000 or later)." /SD IDOK
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
