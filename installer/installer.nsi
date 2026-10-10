Unicode true

!define APPNAME "Heartwire"
!define EXENAME "heartwire.exe"
!define UNINSTALLER "Uninstall.exe"
!define ARPKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Heartwire"
!define RUNKEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define DATADIR "$APPDATA\heartwire"

Name "${APPNAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Heartwire"
InstallDirRegKey HKCU "${ARPKEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define MUI_ICON "..\assets\icon.ico"
!define MUI_UNICON "..\assets\icon.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXENAME}"
!define MUI_FINISHPAGE_RUN_TEXT "Start Heartwire"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

VIProductVersion "${VIVERSION}"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APPNAME} Setup"
VIAddVersionKey "LegalCopyright" ""

!macro GuardRunning
	${Do}
		nsExec::ExecToStack 'cmd /c tasklist /NH /FI "IMAGENAME eq ${EXENAME}" | find /I "${EXENAME}"'
		Pop $0
		Pop $1
		${If} $0 <> 0
			${Break}
		${EndIf}
		${If} ${Silent}
			SetErrorLevel 5
			Quit
		${EndIf}
		MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "${APPNAME} is running. Close it, then try again." IDRETRY +2
		Quit
	${Loop}
!macroend

Function .onInit
	!insertmacro GuardRunning
FunctionEnd

Function un.onInit
	!insertmacro GuardRunning
FunctionEnd

Section "Install"
	ReadRegStr $0 HKCU "${ARPKEY}" "InstallLocation"
	${If} $0 != ""
	${AndIf} $0 != $INSTDIR
	${AndIf} ${FileExists} "$0\${UNINSTALLER}"
		ReadRegStr $2 HKCU "${RUNKEY}" "${APPNAME}"
		DetailPrint "Removing the previous version from $0"
		ExecWait '"$0\${UNINSTALLER}" /S _?=$0' $1
		${If} $1 != 0
			DetailPrint "The previous uninstaller returned $1"
		${EndIf}
		Delete "$0\${UNINSTALLER}"
		RMDir "$0"
		${If} $2 == '"$0\${EXENAME}" --minimized'
			WriteRegStr HKCU "${RUNKEY}" "${APPNAME}" '"$INSTDIR\${EXENAME}" --minimized'
		${EndIf}
	${EndIf}

	SetOutPath "$INSTDIR\firmware"
	File "${PAYLOAD}\firmware\main.py"
	File "${PAYLOAD}\firmware\hr_config.example.py"
	SetOutPath "$INSTDIR"
	File "${PAYLOAD}\${EXENAME}"
	File "${PAYLOAD}\README.md"
	File "${PAYLOAD}\LICENSE"
	File "${PAYLOAD}\NOTICE.md"
	WriteUninstaller "$INSTDIR\${UNINSTALLER}"
	CreateShortcut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\${EXENAME}"

	WriteRegStr HKCU "${ARPKEY}" "DisplayName" "${APPNAME}"
	WriteRegStr HKCU "${ARPKEY}" "DisplayVersion" "${VERSION}"
	WriteRegStr HKCU "${ARPKEY}" "DisplayIcon" "$INSTDIR\${EXENAME}"
	WriteRegStr HKCU "${ARPKEY}" "Publisher" "RealWhyKnot"
	WriteRegStr HKCU "${ARPKEY}" "InstallLocation" "$INSTDIR"
	WriteRegStr HKCU "${ARPKEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER}"'
	WriteRegStr HKCU "${ARPKEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER}" /S'
	WriteRegStr HKCU "${ARPKEY}" "URLInfoAbout" "https://github.com/RealWhyKnot/heartwire"
	WriteRegDWORD HKCU "${ARPKEY}" "NoModify" 1
	WriteRegDWORD HKCU "${ARPKEY}" "NoRepair" 1
	${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
	WriteRegDWORD HKCU "${ARPKEY}" "EstimatedSize" $0
SectionEnd

Section "Uninstall"
	ReadRegStr $0 HKCU "${RUNKEY}" "${APPNAME}"
	${If} $0 == '"$INSTDIR\${EXENAME}" --minimized'
		DeleteRegValue HKCU "${RUNKEY}" "${APPNAME}"
	${EndIf}

	Delete "$SMPROGRAMS\${APPNAME}.lnk"
	DeleteRegKey HKCU "${ARPKEY}"
	Delete "$INSTDIR\${EXENAME}"
	Delete "$INSTDIR\README.md"
	Delete "$INSTDIR\LICENSE"
	Delete "$INSTDIR\NOTICE.md"
	Delete "$INSTDIR\firmware\main.py"
	Delete "$INSTDIR\firmware\hr_config.example.py"
	RMDir "$INSTDIR\firmware"
	Delete "$INSTDIR\heartwire.vrmanifest"
	Delete "$INSTDIR\heartwire-icon.png"
	Delete "$INSTDIR\${UNINSTALLER}"
	RMDir "$INSTDIR"
	RMDir /r "${DATADIR}\update"

	${IfNot} ${Silent}
		MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Also delete your Heartwire settings and log?$\r$\n$\r$\n${DATADIR}" IDNO keep_data
		RMDir /r "${DATADIR}"
		keep_data:
	${EndIf}
SectionEnd
