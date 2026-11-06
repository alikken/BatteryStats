; Установщик BatteryStats (NSIS 3, Unicode).
; Сборка: makensis -DVERSION=1.1.0 -DAPP_EXE=путь\BatteryStats.exe -DOUTFILE=BatteryStats-Setup-1.1.0.exe installer.nsi

Target amd64-unicode
SetCompressor /SOLID lzma
ManifestDPIAware true

!include "MUI2.nsh"
!include "x64.nsh"
!include "WinVer.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "WinMessages.nsh"

!ifndef VERSION
  !error "Не задана VERSION"
!endif
!ifndef APP_EXE
  !error "Не задан APP_EXE"
!endif
!ifndef OUTFILE
  !define OUTFILE "BatteryStats-Setup-${VERSION}.exe"
!endif

!define APPNAME   "BatteryStats"
!define PUBLISHER "alikken"
!define APPKEY    "Software\BatteryStats"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\BatteryStats"
!define RUNKEY    "Software\Microsoft\Windows\CurrentVersion\Run"

Name "${APPNAME} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\BatteryStats"
InstallDirRegKey HKCU "${APPKEY}" "InstallDir"
RequestExecutionLevel user
BrandingText "${APPNAME} ${VERSION} · by ${PUBLISHER}"
ShowInstDetails nevershow
ShowUninstDetails nevershow

; ---------- Интерфейс ----------
!define MUI_ICON   "../res/app.ico"
!define MUI_UNICON "../res/app.ico"
!define MUI_ABORTWARNING
!define MUI_WELCOMEFINISHPAGE_BITMAP "../res/installer-side.bmp"
!define MUI_UNWELCOMEFINISHPAGE_BITMAP "../res/installer-side.bmp"
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_RIGHT
!define MUI_HEADERIMAGE_BITMAP "../res/installer-header.bmp"
!define MUI_HEADERIMAGE_UNBITMAP "../res/installer-header.bmp"
!define MUI_WELCOMEPAGE_TITLE "$(WelcomeTitle)"
!define MUI_WELCOMEPAGE_TEXT  "$(WelcomeText)"
!define MUI_COMPONENTSPAGE_SMALLDESC
!define MUI_FINISHPAGE_RUN "$INSTDIR\BatteryStats.exe"
!define MUI_FINISHPAGE_RUN_TEXT "$(RunNow)"
!define MUI_FINISHPAGE_TEXT "$(FinishText)"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "Russian"
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Polish"

; ---------- Тексты ----------
LangString WelcomeTitle ${LANG_RUSSIAN} "Установка BatteryStats ${VERSION}"
LangString WelcomeTitle ${LANG_ENGLISH} "BatteryStats ${VERSION} Setup"
LangString WelcomeText ${LANG_RUSSIAN} "BatteryStats показывает в трее Windows, сколько ватт потребляет ноутбук от батареи: мощность, напряжение, ток, график, самые энергоёмкие программы и отчёт о расходе за месяц.$\r$\n$\r$\nПрава администратора не нужны — программа устанавливается только для текущего пользователя.$\r$\n$\r$\nНажмите «Далее», чтобы продолжить."
LangString WelcomeText ${LANG_ENGLISH} "BatteryStats shows in the Windows tray how many watts your laptop draws from the battery: power, voltage, current, a chart, the most power-hungry apps and a monthly usage report.$\r$\n$\r$\nNo administrator rights needed — it installs for the current user only.$\r$\n$\r$\nClick Next to continue."
LangString RunNow ${LANG_RUSSIAN} "Запустить BatteryStats"
LangString RunNow ${LANG_ENGLISH} "Run BatteryStats"
LangString FinishText ${LANG_RUSSIAN} "BatteryStats установлен. Значок с мощностью появится в трее рядом с часами. Если его не видно, нажмите стрелку ^ в трее и перетащите значок на панель задач."
LangString FinishText ${LANG_ENGLISH} "BatteryStats is installed. The power icon appears in the tray next to the clock. If you can't see it, click the ^ arrow in the tray and drag the icon onto the taskbar."
LangString SecMainName ${LANG_RUSSIAN} "BatteryStats (обязательно)"
LangString SecMainName ${LANG_ENGLISH} "BatteryStats (required)"
LangString SecMainDesc ${LANG_RUSSIAN} "Программа и ярлык в меню «Пуск»."
LangString SecMainDesc ${LANG_ENGLISH} "The app and a Start menu shortcut."
LangString SecDesktopName ${LANG_RUSSIAN} "Ярлык на рабочем столе"
LangString SecDesktopName ${LANG_ENGLISH} "Desktop shortcut"
LangString SecDesktopDesc ${LANG_RUSSIAN} "Создать ярлык BatteryStats на рабочем столе."
LangString SecDesktopDesc ${LANG_ENGLISH} "Create a BatteryStats shortcut on the desktop."
LangString SecAutoName ${LANG_RUSSIAN} "Запускать при входе в Windows"
LangString SecAutoName ${LANG_ENGLISH} "Start with Windows"
LangString SecAutoDesc ${LANG_RUSSIAN} "BatteryStats будет стартовать автоматически. Можно отключить позже в меню значка."
LangString SecAutoDesc ${LANG_ENGLISH} "Start BatteryStats automatically. You can turn this off later from the tray menu."
LangString Need64 ${LANG_RUSSIAN} "BatteryStats работает только на 64-битной Windows."
LangString Need64 ${LANG_ENGLISH} "BatteryStats requires 64-bit Windows."
LangString NeedWin10 ${LANG_RUSSIAN} "Для BatteryStats нужна Windows 10 или Windows 11."
LangString NeedWin10 ${LANG_ENGLISH} "BatteryStats requires Windows 10 or Windows 11."
LangString WelcomeTitle ${LANG_POLISH} "Instalacja BatteryStats ${VERSION}"
LangString WelcomeText ${LANG_POLISH} "BatteryStats pokazuje w zasobniku Windows, ile watów laptop pobiera z baterii: moc, napięcie, prąd, wykres i najbardziej energożerne aplikacje, a także miesięczny raport zużycia.$\r$\n$\r$\nNie są potrzebne uprawnienia administratora — program instaluje się tylko dla bieżącego użytkownika.$\r$\n$\r$\nKliknij „Dalej”, aby kontynuować."
LangString RunNow ${LANG_POLISH} "Uruchom BatteryStats"
LangString FinishText ${LANG_POLISH} "BatteryStats został zainstalowany. Ikona z mocą pojawi się w zasobniku obok zegara. Jeśli jej nie widać, kliknij strzałkę ^ w zasobniku i przeciągnij ikonę na pasek zadań."
LangString SecMainName ${LANG_POLISH} "BatteryStats (wymagane)"
LangString SecMainDesc ${LANG_POLISH} "Program i skrót w menu Start."
LangString SecDesktopName ${LANG_POLISH} "Skrót na pulpicie"
LangString SecDesktopDesc ${LANG_POLISH} "Utwórz skrót BatteryStats na pulpicie."
LangString SecAutoName ${LANG_POLISH} "Uruchamiaj razem z Windows"
LangString SecAutoDesc ${LANG_POLISH} "BatteryStats będzie uruchamiany automatycznie. Można to wyłączyć później w menu ikony."
LangString Need64 ${LANG_POLISH} "BatteryStats wymaga 64-bitowego systemu Windows."
LangString NeedWin10 ${LANG_POLISH} "BatteryStats wymaga systemu Windows 10 lub Windows 11."
LangString AskData ${LANG_RUSSIAN} "Удалить также собранную статистику расхода батареи и отчёты?"
LangString AskData ${LANG_ENGLISH} "Also delete the collected battery usage statistics and reports?"
LangString AskData ${LANG_POLISH} "Usunąć również zebrane statystyki zużycia baterii i raporty?"


; ---------- Сведения о файле установщика ----------
VIProductVersion "${VERSION}.0"
VIFileVersion "${VERSION}.0"
VIAddVersionKey /LANG=${LANG_RUSSIAN} "ProductName" "${APPNAME}"
VIAddVersionKey /LANG=${LANG_RUSSIAN} "FileDescription" "Установщик ${APPNAME}"
VIAddVersionKey /LANG=${LANG_RUSSIAN} "CompanyName" "${PUBLISHER}"
VIAddVersionKey /LANG=${LANG_RUSSIAN} "LegalCopyright" "© 2026 ${PUBLISHER}"
VIAddVersionKey /LANG=${LANG_RUSSIAN} "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=${LANG_RUSSIAN} "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "ProductName" "${APPNAME}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "FileDescription" "${APPNAME} Setup"
VIAddVersionKey /LANG=${LANG_ENGLISH} "CompanyName" "${PUBLISHER}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "LegalCopyright" "© 2026 ${PUBLISHER}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=${LANG_ENGLISH} "ProductVersion" "${VERSION}"

; ---------- Закрыть запущенную копию (обновление/удаление) ----------
!macro CLOSE_RUNNING UN
Function ${UN}CloseRunning
  StrCpy $1 0
  loop:
    FindWindow $0 "BatteryStatsMain" ""
    ${If} $0 = 0
      Goto done
    ${EndIf}
    SendMessage $0 ${WM_CLOSE} 0 0 /TIMEOUT=2000
    Sleep 300
    IntOp $1 $1 + 1
    ${If} $1 < 20
      Goto loop
    ${EndIf}
  done:
  Sleep 300
FunctionEnd
!macroend
!insertmacro CLOSE_RUNNING ""
!insertmacro CLOSE_RUNNING "un."

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "$(Need64)"
    Abort
  ${EndIf}
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_ICONSTOP "$(NeedWin10)"
    Abort
  ${EndIf}
FunctionEnd

; ---------- Установка ----------
Section "$(SecMainName)" SecMain
  SectionIn RO
  Call CloseRunning
  SetOutPath "$INSTDIR"
  SetOverwrite on
  File "/oname=BatteryStats.exe" "${APP_EXE}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateShortcut "$SMPROGRAMS\BatteryStats.lnk" "$INSTDIR\BatteryStats.exe" "" "$INSTDIR\BatteryStats.exe" 0

  WriteRegStr HKCU "${APPKEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${APPKEY}" "Version" "${VERSION}"

  WriteRegStr   HKCU "${UNINSTKEY}" "DisplayName" "${APPNAME}"
  WriteRegStr   HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr   HKCU "${UNINSTKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr   HKCU "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\BatteryStats.exe,0"
  WriteRegStr   HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr   HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr   HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" "$0"

  ; Автозапуск решает выбор в установщике (секция ниже добавит заново).
  DeleteRegValue HKCU "${RUNKEY}" "BatteryStats"
SectionEnd

Section /o "$(SecDesktopName)" SecDesktop
  CreateShortcut "$DESKTOP\BatteryStats.lnk" "$INSTDIR\BatteryStats.exe" "" "$INSTDIR\BatteryStats.exe" 0
SectionEnd

Section "$(SecAutoName)" SecAuto
  WriteRegStr HKCU "${RUNKEY}" "BatteryStats" '"$INSTDIR\BatteryStats.exe"'
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "$(SecMainDesc)"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "$(SecDesktopDesc)"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAuto} "$(SecAutoDesc)"
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; ---------- Удаление ----------
Section "Uninstall"
  Call un.CloseRunning
  Delete "$INSTDIR\BatteryStats.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\BatteryStats.lnk"
  Delete "$DESKTOP\BatteryStats.lnk"
  DeleteRegValue HKCU "${RUNKEY}" "BatteryStats"
  DeleteRegKey HKCU "${UNINSTKEY}"
  DeleteRegKey HKCU "${APPKEY}"
  ; Статистика и отчёты — по выбору (в тихом режиме сохраняются).
  IfFileExists "$LOCALAPPDATA\BatteryStats\*.*" 0 +3
    MessageBox MB_YESNO|MB_ICONQUESTION "$(AskData)" /SD IDNO IDNO +2
    RMDir /r "$LOCALAPPDATA\BatteryStats"
SectionEnd
