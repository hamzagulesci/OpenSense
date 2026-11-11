; Inno Setup script for the OpenSense installer.
; Build: iscc /DAppVersion=0.2.0 installer\opensense.iss
; (expects target\release\opensense.exe and opensense-cli.exe)

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define Root AddBackslash(SourcePath) + ".."

[Setup]
AppId={{6F1D3C2A-8E4B-4F7A-9C55-0B2E7D41A9E3}
AppName=OpenSense
AppVersion={#AppVersion}
AppVerName=OpenSense {#AppVersion}
AppPublisher=OpenSense contributors
AppPublisherURL=https://github.com/hamzagulesci/OpenSense
AppSupportURL=https://github.com/hamzagulesci/OpenSense/issues
AppUpdatesURL=https://github.com/hamzagulesci/OpenSense/releases
DefaultDirName={autopf}\OpenSense
DefaultGroupName=OpenSense
DisableProgramGroupPage=yes
LicenseFile={#Root}\LICENSE
OutputDir={#Root}\dist
OutputBaseFilename=OpenSense-v{#AppVersion}-setup
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; The setup asks for the language (Windows' language preselected) and
; OpenSense then uses the same one.
ShowLanguageDialog=yes
LanguageDetectionMethod=uilanguage
UninstallDisplayIcon={app}\opensense.exe
UninstallDisplayName=OpenSense
CloseApplications=no
#ifexist AddBackslash(SourcePath) + "opensense.ico"
SetupIconFile=opensense.ico
#endif

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "tr"; MessagesFile: "compiler:Languages\Turkish.isl"

[CustomMessages]
en.StartWithWindows=Start OpenSense with Windows
tr.StartWithWindows=OpenSense'i Windows ile başlat
en.LaunchNow=Launch OpenSense
tr.LaunchNow=OpenSense'i başlat

[Tasks]
Name: "autostart"; Description: "{cm:StartWithWindows}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Root}\target\release\opensense.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\target\release\opensense-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "{#Root}\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\OpenSense"; Filename: "{app}\opensense.exe"
Name: "{group}\{cm:UninstallProgram,OpenSense}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\OpenSense"; Filename: "{app}\opensense.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\opensense.exe"; Parameters: "--set-language {language}"; Flags: runhidden waituntilterminated
Filename: "{app}\opensense.exe"; Parameters: "--autostart-on"; Flags: runhidden waituntilterminated; Tasks: autostart
Filename: "{app}\opensense.exe"; Parameters: "--autostart-off"; Flags: runhidden waituntilterminated; Tasks: not autostart
Filename: "{app}\opensense.exe"; Description: "{cm:LaunchNow}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; A clean quit first: the agent restores the fans before it exits.
Filename: "{app}\opensense.exe"; Parameters: "--quit"; Flags: runhidden waituntilterminated; RunOnceId: "QuitApp"
Filename: "{app}\opensense.exe"; Parameters: "--autostart-off"; Flags: runhidden waituntilterminated; RunOnceId: "Autostart"
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM opensense.exe"; Flags: runhidden waituntilterminated; RunOnceId: "StopApp"

[UninstallDelete]
; Left behind by the in-app updater.
Type: files; Name: "{app}\*.old"
Type: files; Name: "{app}\*.old.*"

[Code]
// Stop a running OpenSense (tray agent and windows) so its files can be
// replaced. A clean quit first lets the agent restore the fans; taskkill
// only removes what is left (open windows, an older version).
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  if FileExists(ExpandConstant('{app}\opensense.exe')) then
    Exec(ExpandConstant('{app}\opensense.exe'), '--quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM opensense.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(500);
  Result := '';
end;
