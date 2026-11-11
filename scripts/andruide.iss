; AndruIDE's Windows installer (Inno Setup 6), made by scripts\package-windows.ps1
; from dist\windows\AndruIDE. Installs for this user (no administrator needed)
; in %LOCALAPPDATA%\Programs\AndruIDE, with a Start menu entry, and optionally a
; desktop shortcut and `andruide` on the PATH. The AppId is the one from when it
; was called AndruIDE, so it updates that install in its folder.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
; x64, or arm64 for Windows on Arm (which also runs the x64 one).
#ifndef Arch
  #define Arch "x64"
#endif

[Setup]
AppId={{6F0B8E52-3C8E-4C8C-9E5B-6C4D2A9B7E31}
AppName=AndruIDE
AppVersion={#AppVersion}
AppVerName=AndruIDE {#AppVersion}
AppPublisher=Lumary Labs
AppPublisherURL=https://muxin.dev
DefaultDirName={localappdata}\Programs\AndruIDE
DefaultGroupName=AndruIDE
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=AndruIDE-Setup-{#Arch}
SetupIconFile=..\assets\brand\andruide.ico
UninstallDisplayIcon={app}\andruide.exe
Compression=lzma2/max
SolidCompression=yes
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
WizardStyle=modern
ChangesEnvironment=yes
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Add a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "addtopath"; Description: "Add the andruide command to the PATH (for terminals and AI agents)"; GroupDescription: "Command line:"

[Files]
Source: "..\dist\windows\AndruIDE\andruide.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\windows\AndruIDE\agents\*"; DestDir: "{app}\agents"; Flags: ignoreversion
Source: "..\dist\windows\AndruIDE\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\windows\AndruIDE\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\windows\AndruIDE\NOTICE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\windows\AndruIDE\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

; Old names (muxin, and before that muxen): their program and shortcuts.
[InstallDelete]
Type: files; Name: "{app}\muxin.exe"
Type: files; Name: "{app}\muxen.exe"
Type: files; Name: "{autoprograms}\Muxin.lnk"
Type: files; Name: "{autodesktop}\Muxin.lnk"
Type: files; Name: "{autoprograms}\Muxen.lnk"
Type: files; Name: "{autodesktop}\Muxen.lnk"

; Share links (andruide://join...) open in AndruIDE. The scheme keeps its name.
[Registry]
Root: HKCU; Subkey: "Software\Classes\andruide"; ValueType: string; ValueName: ""; ValueData: "URL:AndruIDE share link"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\andruide"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\andruide\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\andruide.exe,0"
Root: HKCU; Subkey: "Software\Classes\andruide\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\andruide.exe"" ""%1"""

[Icons]
Name: "{autoprograms}\AndruIDE"; Filename: "{app}\andruide.exe"
Name: "{autodesktop}\AndruIDE"; Filename: "{app}\andruide.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\andruide.exe"; Description: "Open AndruIDE"; Flags: nowait postinstall skipifsilent

[Code]
const
  EnvKey = 'Environment';

// A AndruIDE kept running by its tray icon has no window to close, and its files
// are in use: ask it to quit. (One older than this doesn't know `quit`, and has
// a window for CloseApplications.)
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  if Exec(ExpandConstant('{app}\andruide.exe'), 'quit', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
    Sleep(1000);
  Result := '';
end;

function PathHas(Dir: string): Boolean;
var
  Path: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path) then
    Path := '';
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Path) + ';') > 0;
end;

procedure AddToPath(Dir: string);
var
  Path: string;
begin
  if PathHas(Dir) then
    exit;
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path) then
    Path := '';
  if (Path <> '') and (Copy(Path, Length(Path), 1) <> ';') then
    Path := Path + ';';
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path + Dir);
end;

procedure RemoveFromPath(Dir: string);
var
  Path: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path) then
    exit;
  P := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Path) + ';');
  if P = 0 then
    exit;
  Delete(Path, P, Length(Dir) + 1);
  if (Length(Path) > 0) and (Copy(Path, Length(Path), 1) = ';') then
    Delete(Path, Length(Path), 1);
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddToPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
