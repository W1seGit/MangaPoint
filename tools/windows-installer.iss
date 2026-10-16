; The Windows installer for MangaPoint, built with Inno Setup (https://jrsoftware.org/isinfo.php).
;
;   ISCC /DAppVersion=1.0.0 /DSourceExe=path\to\mangapoint.exe /DOutputDir=dist tools\windows-installer.iss
;
; It installs for the current user by default (no administrator prompt), adds a
; Start menu entry (and, if asked, a desktop icon), and an entry in "Apps &
; features" with an uninstaller. Your library and settings, in
; %APPDATA%\MangaPoint, are left alone when it is uninstalled.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\mangapoint.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

[Setup]
; Never change this: it is how Windows knows a newer installer is the same app.
AppId={{F0405E89-44D7-409B-8382-BBBE904904C2}
AppName=MangaPoint
AppVersion={#AppVersion}
AppVerName=MangaPoint {#AppVersion}
AppPublisher=MangaPoint contributors
AppPublisherURL=https://github.com/W1seGit/MangaPoint
AppSupportURL=https://github.com/W1seGit/MangaPoint/issues
AppUpdatesURL=https://github.com/W1seGit/MangaPoint/releases
DefaultDirName={autopf}\MangaPoint
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=MangaPoint-{#AppVersion}-windows-x86_64-setup
SetupIconFile=..\apps\desktop\assets\icon.ico
UninstallDisplayIcon={app}\MangaPoint.exe
UninstallDisplayName=MangaPoint
LicenseFile=..\LICENSE
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop icon"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "MangaPoint.exe"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\MangaPoint"; Filename: "{app}\MangaPoint.exe"
Name: "{autodesktop}\MangaPoint"; Filename: "{app}\MangaPoint.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\MangaPoint.exe"; Description: "Start MangaPoint"; Flags: nowait postinstall skipifsilent
; An update started from inside the app runs this installer silently with /RELAUNCH=1,
; and MangaPoint opens again when it is done.
Filename: "{app}\MangaPoint.exe"; Flags: nowait runasoriginaluser; Check: RelaunchRequested

[Code]
// MangaPoint draws its windows with Microsoft Edge WebView2. Windows 11 has it;
// an older Windows 10 may not, so say so rather than leave a window that never opens.
function WebView2Installed: Boolean;
var
  version: String;
begin
  Result :=
    RegQueryStringValue(HKLM32, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', version) or
    RegQueryStringValue(HKCU, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', version);
  Result := Result and (version <> '') and (version <> '0.0.0.0');
end;

function RelaunchRequested: Boolean;
begin
  Result := ExpandConstant('{param:RELAUNCH|0}') = '1';
end;

function InitializeSetup: Boolean;
begin
  Result := True;
  if (not WebView2Installed) and (not WizardSilent) then
    MsgBox('MangaPoint needs the Microsoft Edge WebView2 runtime, which does not seem to be installed on this computer.' + #13#10 + #13#10 +
           'Setup will go on, but install the runtime too (search for "WebView2 Runtime" on microsoft.com) or MangaPoint will not open.',
           mbInformation, MB_OK);
end;
