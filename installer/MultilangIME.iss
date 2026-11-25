; Multilang IME setup (Inno Setup 6).
;   ISCC /DVersion=0.1.0 /DPkgDir=<dist\MultilangIME-0.1.0> /DOutDir=<dist> installer\MultilangIME.iss
; Built by installer\build-release.ps1 -Setup.
;
; A thin wrapper around the tested scripts: it unpacks the release folder to a temporary folder and runs
; install.ps1 with the input language picked here. install.ps1 does the real work (Program Files copy,
; manifest-based file handling, TSF registration, scheduled task, Programs and Features entry). Removal stays
; with uninstall.ps1 through that Programs and Features entry, so this setup registers no uninstaller of its own.
; Silent install: MultilangIME-<ver>-setup.exe /VERYSILENT /INPUTLANG=ko|ja|en

#ifndef Version
  #define Version "0.2.4"
#endif
#ifndef PkgDir
  #define PkgDir "..\dist\MultilangIME-" + Version
#endif
#ifndef OutDir
  #define OutDir "..\dist"
#endif
; file version fields take numbers only: 0.0.0-dev.6 -> 0.0.0 (the full string goes into the text version)
#if Pos("-", Version) > 0
  #define NumVersion Copy(Version, 1, Pos("-", Version) - 1)
#else
  #define NumVersion Version
#endif

[Setup]
AppId={{6F1D3A52-8C4B-4E0F-9A7D-2B5C1E8F4A63}
AppName=Multilang IME
AppVersion={#Version}
AppVerName=Multilang IME {#Version}
AppPublisher=Multilang IME contributors
AppPublisherURL=https://github.com/hoong0515/Multilang-ime
AppSupportURL=https://github.com/hoong0515/Multilang-ime/issues
VersionInfoVersion={#NumVersion}
VersionInfoProductName=Multilang IME
VersionInfoProductVersion={#NumVersion}
VersionInfoProductTextVersion={#Version}
VersionInfoDescription=Multilang IME setup
VersionInfoCopyright=GPL-3.0-or-later
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
CreateAppDir=no
Uninstallable=no
DisableProgramGroupPage=yes
DisableReadyPage=no
LicenseFile=..\LICENSE
SetupIconFile=..\res\multilang.ico
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir={#OutDir}
OutputBaseFilename=MultilangIME-{#Version}-setup
ShowLanguageDialog=auto

[Languages]
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[CustomMessages]
korean.LangPageTitle=입력 언어
korean.LangPageSub=Windows 입력 방법 목록에 어느 언어로 등록할지 고르세요.
korean.LangPageInfo=세 언어(한국어·영어·일본어)는 어느 쪽을 골라도 모두 자동으로 판정합니다. 고른 언어의 입력 방법 목록과 작업 표시줄에 나타납니다.
korean.LangKo=한국어 (권장: 한국어 키보드)
korean.LangJa=日本語
korean.LangEn=English
korean.Updating=설치된 버전 %1을(를) 이 버전으로 바꿉니다. 설정과 학습 기록은 그대로 남습니다.
korean.Running=Multilang IME 설치 스크립트를 실행하는 중...
korean.Failed=설치하지 못했습니다 (코드 %1).%n%n%2%n전체 기록: %3
korean.Done=Multilang IME를 설치했습니다.%n%n이미 열려 있던 프로그램은 다시 열어야 새 입력기가 적용됩니다. 제거는 설정 > 앱에서 할 수 있습니다.%n%n작업 표시줄 검색창: 이 판은 서명이 없어 검색창에서는 후보창이 가려집니다(입력은 됨). 설치 폴더의 local-signing.ps1로 이 PC 전용 인증서를 만들어 켤 수 있지만, 이 PC가 신뢰하는 루트 인증서를 추가하는 일이라 보안상 위험이 있습니다. 필요할 때만, 설치 폴더 README의 안내를 읽고 쓰세요.
english.LangPageTitle=Input language
english.LangPageSub=Choose the language under which Multilang IME is added to your input methods.
english.LangPageInfo=Korean, English and Japanese are all detected automatically whichever you pick. The IME appears in this language's input method list and on the taskbar.
english.LangKo=Korean (recommended for Korean keyboards)
english.LangJa=Japanese
english.LangEn=English
english.Updating=Installed version %1 will be replaced by this version. Settings and learning data are kept.
english.Running=Running the Multilang IME install script...
english.Failed=Setup failed (code %1).%n%n%2%nFull log: %3
english.Done=Multilang IME is installed.%n%nPrograms that were already open need a restart to use the new IME. Remove it from Settings > Apps.%n%nTaskbar search box: this release is unsigned, so the candidate window stays hidden there (typing works). local-signing.ps1 in the install folder can enable it with a certificate made for this PC only, but that adds a trusted root certificate to this PC, which is a security risk. Use it only if you need it, after reading the README in the install folder.
japanese.LangPageTitle=入力言語
japanese.LangPageSub=Multilang IME をどの言語の入力方式として登録するか選んでください。
japanese.LangPageInfo=どれを選んでも韓国語・英語・日本語はすべて自動で判定されます。選んだ言語の入力方式一覧とタスクバーに表示されます。
japanese.LangKo=韓国語
japanese.LangJa=日本語 (日本語キーボード向け)
japanese.LangEn=英語
japanese.Updating=インストール済みのバージョン %1 をこのバージョンに置き換えます。設定と学習データはそのまま残ります。
japanese.Running=Multilang IME のインストールスクリプトを実行しています...
japanese.Failed=インストールできませんでした (コード %1)。%n%n%2%n全体のログ: %3
japanese.Done=Multilang IME をインストールしました。%n%n開いていたプログラムは再起動すると新しい IME が使えます。削除は「設定 > アプリ」から行えます。%n%nタスクバーの検索ボックス: このバージョンは署名されていないため、検索ボックスでは候補ウィンドウが隠れます (入力はできます)。インストールフォルダーの local-signing.ps1 でこの PC 専用の証明書を作って有効にできますが、この PC が信頼するルート証明書を追加するため、セキュリティ上のリスクがあります。必要なときだけ、インストールフォルダーの README を読んでから使ってください。

[Messages]
korean.FinishedLabelNoIcons=Multilang IME 설치가 끝났습니다.
english.FinishedLabelNoIcons=Multilang IME setup has finished.
japanese.FinishedLabelNoIcons=Multilang IME のセットアップが完了しました。

[Files]
; the release folder (the same files as the zip) into a temporary folder that setup deletes when it ends
Source: "{#PkgDir}\*"; DestDir: "{tmp}\pkg"; Flags: recursesubdirs createallsubdirs ignoreversion

[Code]
var
  LangPage: TInputOptionWizardPage;
  InstallOk: Boolean;

function InstalledVersion(var V: String): Boolean;
begin
  Result := RegQueryStringValue(HKLM64, 'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MultilangIME', 'DisplayVersion', V);
end;

function ParamLang(): String;
begin
  Result := Lowercase(ExpandConstant('{param:INPUTLANG|}'));
  if (Result <> 'ko') and (Result <> 'ja') and (Result <> 'en') then Result := '';
end;

procedure InitializeWizard();
var
  V, Info: String;
begin
  Info := CustomMessage('LangPageInfo');
  if InstalledVersion(V) then Info := Info + #13#10#13#10 + FmtMessage(CustomMessage('Updating'), [V]);
  LangPage := CreateInputOptionPage(wpLicense, CustomMessage('LangPageTitle'), CustomMessage('LangPageSub'), Info, True, False);
  LangPage.Add(CustomMessage('LangKo'));
  LangPage.Add(CustomMessage('LangJa'));
  LangPage.Add(CustomMessage('LangEn'));
  if ParamLang() = 'ja' then LangPage.SelectedValueIndex := 1
  else if ParamLang() = 'en' then LangPage.SelectedValueIndex := 2
  else if ParamLang() = 'ko' then LangPage.SelectedValueIndex := 0
  else if ActiveLanguage() = 'japanese' then LangPage.SelectedValueIndex := 1
  else if ActiveLanguage() = 'english' then LangPage.SelectedValueIndex := 2
  else LangPage.SelectedValueIndex := 0;
end;

function SelectedLang(): String;
begin
  case LangPage.SelectedValueIndex of
    1: Result := 'ja';
    2: Result := 'en';
  else
    Result := 'ko';
  end;
end;

function Quote(S: String): String;
begin
  StringChangeEx(S, '''', '''''', True);
  Result := '''' + S + '''';
end;

function LastLines(Path: String; Count: Integer): String;
var
  Lines: TArrayOfString;
  I, First: Integer;
begin
  Result := '';
  if not LoadStringsFromFile(Path, Lines) then Exit;
  First := GetArrayLength(Lines) - Count;
  if First < 0 then First := 0;
  for I := First to GetArrayLength(Lines) - 1 do Result := Result + Lines[I] + #13#10;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Script, Log, Kept, Params: String;
  Code: Integer;
begin
  if CurStep <> ssPostInstall then Exit;
  Script := ExpandConstant('{tmp}\pkg\install.ps1');
  Log := ExpandConstant('{tmp}\install-log.txt');
  // install.ps1 throws on any failure ($ErrorActionPreference = Stop): exit 1 then, with the error in the log
  // UTF-8, not the UTF-16 that *> writes: LastLines reads the file as 8-bit text, and UTF-16 NULs cut the
  // failure message short (it then showed no log lines and no log path)
  Params := '-NoProfile -ExecutionPolicy Bypass -Command "try { & ' + Quote(Script) + ' -Language ' + SelectedLang() +
    ' *>&1 | Out-File -Encoding utf8 ' + Quote(Log) + '; exit 0 } catch { $_ | Out-File -Append -Encoding utf8 ' + Quote(Log) + '; exit 1 }"';
  WizardForm.StatusLabel.Caption := CustomMessage('Running');
  InstallOk := Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'), Params, ExpandConstant('{tmp}\pkg'),
    SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
  // {tmp} is deleted when setup ends: keep a copy of the script log (success or not) where the user can find it
  Kept := ExpandConstant('{localappdata}\multilang-ime\setup-install.log');
  ForceDirectories(ExtractFileDir(Kept));
  if not FileExists(Log) then SaveStringToFile(Log, 'install.ps1 left no log (exit code ' + IntToStr(Code) + ')' + #13#10, False);
  if not FileCopy(Log, Kept, False) then Kept := Log;
  if not InstallOk then
    SuppressibleMsgBox(FmtMessage(CustomMessage('Failed'), [IntToStr(Code), LastLines(Log, 15), Kept]), mbError, MB_OK, IDOK);
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if (CurPageID = wpFinished) and InstallOk then
    WizardForm.FinishedLabel.Caption := CustomMessage('Done');
end;
