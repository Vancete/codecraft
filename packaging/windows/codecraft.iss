; Inno Setup script for CodeCraft's Windows installer. Built by the release workflow:
;   iscc /DAppVersion=0.1.0 /DOutputName=codecraft-v0.1.0-windows-x86_64-setup packaging\windows\codecraft.iss
; Installs per user (no administrator rights needed) unless the user picks "all users".

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef OutputName
  #define OutputName "codecraft-setup"
#endif
#define Root "..\.."

[Setup]
AppId={{3C8F2A61-7D4E-4B9A-A0E5-5F1C6B8D2E47}
AppName=CodeCraft
AppVersion={#AppVersion}
AppVerName=CodeCraft {#AppVersion}
AppPublisher=Iván
AppPublisherURL=https://github.com/Vancete/codecraft
AppSupportURL=https://github.com/Vancete/codecraft/issues
AppUpdatesURL=https://github.com/Vancete/codecraft/releases
DefaultDirName={autopf}\CodeCraft
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#Root}\dist
OutputBaseFilename={#OutputName}
SetupIconFile={#Root}\assets\app-icon\codecraft.ico
UninstallDisplayIcon={app}\codecraft.exe
UninstallDisplayName=CodeCraft
LicenseFile={#Root}\LICENSE-MIT
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "filemenu"; Description: "Add ""Open with CodeCraft"" to the right-click menu of files"; GroupDescription: "Explorer:"
Name: "foldermenu"; Description: "Add ""Open in CodeCraft"" to the right-click menu of folders"; GroupDescription: "Explorer:"

[Files]
Source: "{#Root}\target\release\codecraft.exe"; DestDir: "{app}"; Flags: ignoreversion
; The example extensions, next to the executable where CodeCraft looks for them.
Source: "{#Root}\target\release\extensions\*"; DestDir: "{app}\extensions"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Root}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\ATTRIBUTION.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\CodeCraft"; Filename: "{app}\codecraft.exe"
Name: "{autodesktop}\CodeCraft"; Filename: "{app}\codecraft.exe"; Tasks: desktopicon

[Registry]
; Listed in "Open with" for any file (HKCU or HKLM, per install mode).
Root: HKA; Subkey: "Software\Classes\Applications\codecraft.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "CodeCraft"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\codecraft.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\codecraft.exe"" ""%1"""
; "Open with CodeCraft" on any file.
Root: HKA; Subkey: "Software\Classes\*\shell\codecraft"; ValueType: string; ValueName: ""; ValueData: "Open with CodeCraft"; Tasks: filemenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\*\shell\codecraft"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\codecraft.exe"""; Tasks: filemenu
Root: HKA; Subkey: "Software\Classes\*\shell\codecraft\command"; ValueType: string; ValueName: ""; ValueData: """{app}\codecraft.exe"" ""%1"""; Tasks: filemenu
; "Open in CodeCraft" on a folder and on a folder's background.
Root: HKA; Subkey: "Software\Classes\Directory\shell\codecraft"; ValueType: string; ValueName: ""; ValueData: "Open in CodeCraft"; Tasks: foldermenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\shell\codecraft"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\codecraft.exe"""; Tasks: foldermenu
Root: HKA; Subkey: "Software\Classes\Directory\shell\codecraft\command"; ValueType: string; ValueName: ""; ValueData: """{app}\codecraft.exe"" ""%1"""; Tasks: foldermenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\codecraft"; ValueType: string; ValueName: ""; ValueData: "Open in CodeCraft"; Tasks: foldermenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\codecraft"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\codecraft.exe"""; Tasks: foldermenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\codecraft\command"; ValueType: string; ValueName: ""; ValueData: """{app}\codecraft.exe"" ""%V"""; Tasks: foldermenu

[Run]
Filename: "{app}\codecraft.exe"; Description: "{cm:LaunchProgram,CodeCraft}"; Flags: nowait postinstall skipifsilent
