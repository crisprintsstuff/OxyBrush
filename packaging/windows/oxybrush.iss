; Inno Setup script for the OxyBrush Windows installer.
;
; Built in CI by .github/workflows/release.yml:
;   iscc /DAppVersion=0.0.1-alpha /DNumericVersion=0.0.1.0 /DSourceDir=<dir with oxybrush.exe> packaging\windows\oxybrush.iss
;
; Installs per-user by default (no admin prompt); the user may choose an
; all-users install instead. Adds Start-menu (and optional desktop)
; shortcuts, an uninstaller, and the .oxb project-file association.

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#ifndef NumericVersion
  #define NumericVersion "0.0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\target\x86_64-pc-windows-msvc\dist"
#endif
#define RepoRoot "..\.."

[Setup]
; Never change AppId: it identifies the app for upgrades and uninstall.
AppId={{66DE57B5-04C1-4FDB-93CD-ED8EAEA50F14}
AppName=OxyBrush
AppVersion={#AppVersion}
AppVerName=OxyBrush {#AppVersion}
AppPublisher=The OxyBrush Contributors
AppPublisherURL=https://github.com/crisprintsstuff/OxyBrush
AppSupportURL=https://github.com/crisprintsstuff/OxyBrush/issues
AppUpdatesURL=https://github.com/crisprintsstuff/OxyBrush/releases
VersionInfoVersion={#NumericVersion}
VersionInfoProductName=OxyBrush
VersionInfoDescription=OxyBrush installer
DefaultDirName={autopf}\OxyBrush
DefaultGroupName=OxyBrush
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
ChangesAssociations=yes
UninstallDisplayIcon={app}\oxybrush.exe
UninstallDisplayName=OxyBrush {#AppVersion}
SetupIconFile={#RepoRoot}\packaging\icons\oxybrush.ico
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir={#RepoRoot}\dist
OutputBaseFilename=OxyBrush-{#AppVersion}-windows-x86_64-setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "associate"; Description: "Open OxyBrush projects (.oxb) with OxyBrush"; GroupDescription: "File associations:"

[Files]
Source: "{#SourceDir}\oxybrush.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\LICENSE-MIT"; DestDir: "{app}"; DestName: "LICENSE-MIT.txt"; Flags: ignoreversion
Source: "{#RepoRoot}\LICENSE-APACHE"; DestDir: "{app}"; DestName: "LICENSE-APACHE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\OxyBrush"; Filename: "{app}\oxybrush.exe"
Name: "{autodesktop}\OxyBrush"; Filename: "{app}\oxybrush.exe"; Tasks: desktopicon

[Registry]
; .oxb association, written to HKCU or HKLM depending on the install mode.
Root: HKA; Subkey: "Software\Classes\.oxb\OpenWithProgids"; ValueType: string; ValueName: "OxyBrush.Project"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\OxyBrush.Project"; ValueType: string; ValueName: ""; ValueData: "OxyBrush project"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\OxyBrush.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\oxybrush.exe,0"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\OxyBrush.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\oxybrush.exe"" ""%1"""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\oxybrush.exe\SupportedTypes"; ValueType: string; ValueName: ".oxb"; ValueData: ""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\oxybrush.exe\SupportedTypes"; ValueType: string; ValueName: ".obj"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\oxybrush.exe\SupportedTypes"; ValueType: string; ValueName: ".stl"; ValueData: ""

[Run]
Filename: "{app}\oxybrush.exe"; Description: "{cm:LaunchProgram,OxyBrush}"; Flags: nowait postinstall skipifsilent
