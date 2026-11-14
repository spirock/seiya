; Built only from the validated, allowlisted Windows staging directory.
#ifndef StageDir
  #error StageDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif
#ifndef PackageVersion
  #error PackageVersion is required
#endif

[Setup]
AppId=Seiya.Rust.Experimental.Windows.x64
AppName=Seiya Rust Experimental
AppVersion={#PackageVersion}
AppPublisher=Seiya
DefaultDirName={localappdata}\Programs\Seiya Rust Experimental
DefaultGroupName=Seiya Rust Experimental
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0.17763
OutputDir={#OutputDir}
OutputBaseFilename=Seiya-Rust-{#PackageVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
CloseApplications=no
RestartApplications=no
Uninstallable=yes
SignedUninstaller=no

[Files]
Source: "{#StageDir}\seiya-desktop.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\seiya-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\server.discovery.example.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\BUILD-INFO.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\Seiya-Rust-Help.cmd"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\Seiya-Rust.cmd"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\local-models.example.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\LOCAL_MODELS.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\BACKENDS_LOCALES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\IMAGENES_DESDE_CHAT.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\RESIDENCIA_NATIVA.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\PRUEBA_INSTALADOR.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\desktop-chat-images.example.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\local-models-images.example.json"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Seiya Rust Experimental"; Filename: "{app}\seiya-desktop.exe"; Parameters: "--launch"; WorkingDir: "{app}"
Name: "{autoprograms}\Seiya Rust Experimental - Ayuda"; Filename: "{app}\Seiya-Rust-Help.cmd"; WorkingDir: "{app}"
