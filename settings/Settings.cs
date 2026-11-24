// Multilang IME 설정 (최소판). %LOCALAPPDATA%\multilang-ime\settings.json 을 고친다.
// 백엔드는 이 파일을 1초 안에 다시 읽으므로 저장하면 바로 적용된다.
// 빌드: powershell -File settings\build.ps1  (.NET Framework 4.x의 csc, 추가 설치 없음)
//
// Copyright (C) 2026 Multilang IME contributors. GPL-3.0-or-later.
using System;
using System.Collections;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text;
using System.Web.Script.Serialization;
using System.Windows.Forms;

[assembly: System.Reflection.AssemblyTitle("Multilang IME settings")]
[assembly: System.Reflection.AssemblyProduct("Multilang IME")]
[assembly: System.Reflection.AssemblyCompany("Multilang IME contributors")]
[assembly: System.Reflection.AssemblyCopyright("GPL-3.0-or-later")]
[assembly: System.Reflection.AssemblyVersion("0.1.0.0")]
[assembly: System.Reflection.AssemblyFileVersion("0.1.0.0")]
[assembly: System.Reflection.AssemblyInformationalVersion("0.1.0")]

static class Program
{
    [DllImport("user32.dll")]
    static extern bool SetProcessDPIAware();

    [STAThread]
    static void Main(string[] args)
    {
        try { SetProcessDPIAware(); } catch { }
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        // --dict: 사용자 사전 탭으로 연다
        Application.Run(new SettingsForm(Array.IndexOf(args, "--dict") >= 0));
    }
}

class SettingsForm : Form
{
    static readonly string Dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "multilang-ime");
    static readonly string FilePath = Path.Combine(Dir, "settings.json");
    static readonly string DictPath = Path.Combine(Dir, "user_dict.json");

    Dictionary<string, object> data = new Dictionary<string, object>();
    readonly CheckBox ko = new CheckBox { Text = "한국어", AutoSize = true };
    readonly CheckBox ja = new CheckBox { Text = "日本語 (일본어)", AutoSize = true };
    readonly CheckBox en = new CheckBox { Text = "English (항상 사용)", AutoSize = true, Checked = true, Enabled = false };
    readonly RadioButton spaceCommit = new RadioButton { Text = "확정 + 전각 공백 (기본)", AutoSize = true };
    readonly RadioButton spaceContinue = new RadioButton { Text = "확정하지 않고 이어서 변환", AutoSize = true };
    readonly CheckBox focusNotice = new CheckBox { Text = "언어를 고정해 쓸 때, 입력칸에 들어가면 고정 상태를 잠깐 보이기", AutoSize = true };
    readonly ListBox apps = new ListBox { Height = 110, IntegralHeight = false };
    readonly TextBox appName = new TextBox();
    readonly ListView dict = new ListView { View = View.Details, FullRowSelect = true, GridLines = true, HideSelection = false, Size = new Size(420, 330) };
    // 친 키는 IME 없이 그대로 받는다 (영문 키 그대로 등록)
    readonly TextBox dictKeys = new TextBox { Width = 130, ImeMode = ImeMode.Disable };
    readonly TextBox dictText = new TextBox { Width = 170 };

    public SettingsForm(bool openDict = false)
    {
        Text = "Multilang IME 설정";
        try { Icon = System.Drawing.Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }
        Font = new Font("Segoe UI", 9f);
        AutoScaleMode = AutoScaleMode.Font;
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        StartPosition = FormStartPosition.CenterScreen;
        AutoSize = true;
        AutoSizeMode = AutoSizeMode.GrowAndShrink;
        Padding = new Padding(12);

        var outer = new FlowLayoutPanel { FlowDirection = FlowDirection.TopDown, AutoSize = true, WrapContents = false };
        Controls.Add(outer);
        var tabs = new TabControl { Size = new Size(500, 590) };
        outer.Controls.Add(tabs);
        var general = new TabPage("일반") { AutoScroll = true, Padding = new Padding(6) };
        var dictPage = new TabPage("사용자 사전") { Padding = new Padding(6) };
        tabs.TabPages.Add(general);
        tabs.TabPages.Add(dictPage);
        if (openDict) tabs.SelectedTab = dictPage;
        var root = new FlowLayoutPanel { FlowDirection = FlowDirection.TopDown, AutoSize = true, WrapContents = false };
        general.Controls.Add(root);

        root.Controls.Add(Group("사용할 언어 (자동 판정·후보)", Flow(en, ko, ja)));
        root.Controls.Add(Group("일본어 문맥에서 Space", Flow(spaceCommit, spaceContinue)));
        root.Controls.Add(Group("알림", Flow(focusNotice)));

        var add = new Button { Text = "추가", AutoSize = true };
        var browse = new Button { Text = "찾아보기…", AutoSize = true };
        var remove = new Button { Text = "선택 삭제", AutoSize = true };
        appName.Width = 200;
        apps.Width = 360;
        add.Click += (s, e) => AddApp(appName.Text);
        appName.KeyDown += (s, e) => { if (e.KeyCode == Keys.Enter) { AddApp(appName.Text); e.SuppressKeyPress = true; } };
        browse.Click += (s, e) =>
        {
            using (var d = new OpenFileDialog { Filter = "프로그램 (*.exe)|*.exe", Title = "IME를 끌 프로그램" })
                if (d.ShowDialog(this) == DialogResult.OK) AddApp(Path.GetFileName(d.FileName));
        };
        remove.Click += (s, e) => { if (apps.SelectedIndex >= 0) apps.Items.RemoveAt(apps.SelectedIndex); };
        var appBox = new FlowLayoutPanel { FlowDirection = FlowDirection.TopDown, AutoSize = true, WrapContents = false };
        appBox.Controls.Add(new Label { Text = "이 프로그램에서는 IME를 쓰지 않습니다 (실행 파일 이름).", AutoSize = true });
        appBox.Controls.Add(apps);
        appBox.Controls.Add(Row(appName, add, browse, remove));
        root.Controls.Add(Group("자동 꺼짐 프로그램", appBox));

        var clear = new Button { Text = "학습 기록 지우기…", AutoSize = true };
        clear.Click += (s, e) => ClearLearning();
        var learnBox = Flow(new Label { Text = "언어 판정·한자 선택·일본어 변환(Mozc) 학습을 모두 지웁니다.", AutoSize = true }, clear);
        root.Controls.Add(Group("학습 기록", learnBox));

        // 사용자 사전: 친 키 → 변환
        dict.Columns.Add("친 키", 140);
        dict.Columns.Add("변환", 250);
        var dAdd = new Button { Text = "추가", AutoSize = true };
        var dRemove = new Button { Text = "선택 삭제", AutoSize = true };
        dAdd.Click += (s, e) => AddEntry();
        dictText.KeyDown += (s, e) => { if (e.KeyCode == Keys.Enter) { AddEntry(); e.SuppressKeyPress = true; } };
        dRemove.Click += (s, e) => { foreach (ListViewItem it in dict.SelectedItems) dict.Items.Remove(it); };
        dict.KeyDown += (s, e) => { if (e.KeyCode == Keys.Delete) foreach (ListViewItem it in dict.SelectedItems) dict.Items.Remove(it); };
        var dictBox = Flow(
            new Label { Text = "치는 키(로마자, 두벌식 키, 영문)와 그때 나올 변환을 등록합니다.\n후보창 맨 앞 '사전' 묶음에 나오고, 첫 Shift 탭이 그 후보를 고릅니다.", AutoSize = true },
            dict,
            Row(new Label { Text = "친 키", AutoSize = true, Anchor = AnchorStyles.Left }, dictKeys,
                new Label { Text = "변환", AutoSize = true, Anchor = AnchorStyles.Left }, dictText),
            Row(dAdd, dRemove),
            new Label { Text = "예: yrsk → よろしくお願いします, gh → GitHub, dlfma → 이름", AutoSize = true, ForeColor = SystemColors.GrayText });
        dictPage.Controls.Add(dictBox);

        var save = new Button { Text = "저장", AutoSize = true, MinimumSize = new Size(80, 0) };
        var close = new Button { Text = "닫기", AutoSize = true, MinimumSize = new Size(80, 0), DialogResult = DialogResult.Cancel };
        save.Click += (s, e) => { if (Save()) Close(); };
        close.Click += (s, e) => Close();
        CancelButton = close;  // Enter는 입력칸에서 추가로 쓰므로 기본 단추를 두지 않는다
        var path = new Label { Text = Dir, AutoSize = true, ForeColor = SystemColors.GrayText };
        outer.Controls.Add(path);
        outer.Controls.Add(Row(save, close));

        LoadSettings();
    }

    static GroupBox Group(string title, Control content)
    {
        var g = new GroupBox { Text = title, AutoSize = true, AutoSizeMode = AutoSizeMode.GrowAndShrink, Padding = new Padding(8), MinimumSize = new Size(400, 0) };
        content.Dock = DockStyle.Fill;
        g.Controls.Add(content);
        return g;
    }

    static FlowLayoutPanel Flow(params Control[] cs)
    {
        var p = new FlowLayoutPanel { FlowDirection = FlowDirection.TopDown, AutoSize = true, WrapContents = false };
        p.Controls.AddRange(cs);
        return p;
    }

    static FlowLayoutPanel Row(params Control[] cs)
    {
        var p = new FlowLayoutPanel { FlowDirection = FlowDirection.LeftToRight, AutoSize = true, WrapContents = false };
        p.Controls.AddRange(cs);
        return p;
    }

    void AddEntry()
    {
        var k = dictKeys.Text.Trim();
        var t = dictText.Text;
        if (k.Length == 0 || t.Length == 0) return;
        foreach (ListViewItem it in dict.Items)
            if (string.Equals(it.Text, k, StringComparison.OrdinalIgnoreCase) && it.SubItems[1].Text == t) return;
        dict.Items.Add(new ListViewItem(new[] { k, t }));
        dictKeys.Clear();
        dictText.Clear();
        dictKeys.Focus();
    }

    void LoadDict()
    {
        try
        {
            if (!File.Exists(DictPath)) return;
            var text = File.ReadAllText(DictPath, Encoding.UTF8).TrimStart('\uFEFF');
            var list = new JavaScriptSerializer().DeserializeObject(text) as object[];
            if (list == null) return;
            foreach (var o in list)
            {
                var e = o as Dictionary<string, object>;
                if (e == null) continue;
                var k = Get(e, "keys") as string;
                var t = Get(e, "text") as string;
                if (!string.IsNullOrEmpty(k) && !string.IsNullOrEmpty(t)) dict.Items.Add(new ListViewItem(new[] { k, t }));
            }
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, "사용자 사전을 읽지 못했습니다.\n" + ex.Message, Text, MessageBoxButtons.OK, MessageBoxIcon.Warning);
        }
    }

    static void WriteAtomic(string path, string text)
    {
        var tmp = path + ".tmp";
        File.WriteAllText(tmp, text, new UTF8Encoding(false));
        if (File.Exists(path)) File.Replace(tmp, path, null);
        else File.Move(tmp, path);
    }

    void AddApp(string name)
    {
        name = (name ?? "").Trim();
        if (name.Length == 0) return;
        if (!name.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)) name += ".exe";
        foreach (var o in apps.Items)
            if (string.Equals((string)o, name, StringComparison.OrdinalIgnoreCase)) return;
        apps.Items.Add(name);
        appName.Clear();
    }

    void LoadSettings()
    {
        try
        {
            if (File.Exists(FilePath))
            {
                var text = File.ReadAllText(FilePath, Encoding.UTF8).TrimStart('﻿');
                var d = new JavaScriptSerializer().DeserializeObject(text) as Dictionary<string, object>;
                if (d != null) data = d;
            }
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, "설정 파일을 읽지 못해 기본값으로 엽니다.\n" + ex.Message, Text, MessageBoxButtons.OK, MessageBoxIcon.Warning);
        }
        var langs = Get(data, "languages") as Dictionary<string, object>;
        ko.Checked = !(langs != null && Get(langs, "ko") is bool && !(bool)Get(langs, "ko"));
        ja.Checked = !(langs != null && Get(langs, "ja") is bool && !(bool)Get(langs, "ja"));
        var space = Get(data, "ja_space") as string;
        spaceContinue.Checked = space == "continue";
        spaceCommit.Checked = !spaceContinue.Checked;
        focusNotice.Checked = !(Get(data, "focus_notice") is bool && !(bool)Get(data, "focus_notice"));
        var list = Get(data, "disabled_apps") as IEnumerable;
        if (list != null)
            foreach (var o in list)
                if (o is string) apps.Items.Add(o);
        LoadDict();
    }

    static object Get(Dictionary<string, object> d, string k)
    {
        object v;
        return d.TryGetValue(k, out v) ? v : null;
    }

    bool Save()
    {
        // 모르는 항목은 그대로 둔다
        data["languages"] = new Dictionary<string, object> { { "ko", ko.Checked }, { "ja", ja.Checked } };
        data["ja_space"] = spaceContinue.Checked ? "continue" : "commit";
        data["focus_notice"] = focusNotice.Checked;
        var list = new List<string>();
        foreach (var o in apps.Items) list.Add((string)o);
        data["disabled_apps"] = list;
        try
        {
            Directory.CreateDirectory(Dir);
            var ser = new JavaScriptSerializer();
            WriteAtomic(FilePath, ser.Serialize(data));
            var entries = new List<Dictionary<string, object>>();
            foreach (ListViewItem it in dict.Items)
                entries.Add(new Dictionary<string, object> { { "keys", it.Text }, { "text", it.SubItems[1].Text } });
            WriteAtomic(DictPath, ser.Serialize(entries));
            return true;
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, "저장하지 못했습니다.\n" + ex.Message, Text, MessageBoxButtons.OK, MessageBoxIcon.Error);
            return false;
        }
    }

    void ClearLearning()
    {
        var ok = MessageBox.Show(this, "학습 기록을 모두 지울까요? 되돌릴 수 없습니다.", Text, MessageBoxButtons.OKCancel, MessageBoxIcon.Warning);
        if (ok != DialogResult.OK) return;
        string err = null, reply;
        if (Backend.Call("{\"op\":\"clear_learning\"}", out reply, out err))
        {
            // 백엔드가 거부함 (샌드박스·낮은 무결성으로 실행된 경우 등): 파일도 지우지 않는다
            if (reply.Contains("\"error\""))
            {
                MessageBox.Show(this, "백엔드가 학습 지우기를 거부했습니다.\n설정 화면을 관리자 권한이 아닌 보통 권한으로 실행해 주세요.",
                    Text, MessageBoxButtons.OK, MessageBoxIcon.Error);
                return;
            }
            MessageBox.Show(this, "학습 기록을 지웠습니다.", Text, MessageBoxButtons.OK, MessageBoxIcon.Information);
            return;
        }
        // 백엔드가 떠 있지 않으면 파일만 지운다 (Mozc 학습은 다음에 백엔드가 떠 있을 때 다시)
        try
        {
            foreach (var f in new[] { "history.json", "hanja_learn.json" })
            {
                var p = Path.Combine(Dir, f);
                if (File.Exists(p)) File.Delete(p);
            }
            MessageBox.Show(this, "백엔드에 연결하지 못해 판정·한자 학습 파일만 지웠습니다.\n일본어(Mozc) 학습은 IME를 쓰는 중에 다시 지워 주세요.\n(" + err + ")",
                Text, MessageBoxButtons.OK, MessageBoxIcon.Information);
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, "지우지 못했습니다.\n" + ex.Message, Text, MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }
}

static class Backend
{
    // ipc::pipe_name 과 같은 규칙: 사용자 SID의 영숫자 외 글자는 '_'
    static string PipeName()
    {
        var sb = new StringBuilder("multilang-ime.");
        foreach (var c in System.Security.Principal.WindowsIdentity.GetCurrent().User.Value)
            sb.Append(c < 128 && char.IsLetterOrDigit(c) ? c : '_');
        return sb.ToString();
    }

    // ipc::win::pipe_owner_trusted 와 같은 확인: 파이프 소유자가 이 사용자이거나 Administrators
    static bool OwnerTrusted(NamedPipeClientStream p)
    {
        var owner = p.GetAccessControl().GetOwner(typeof(System.Security.Principal.SecurityIdentifier))
            as System.Security.Principal.SecurityIdentifier;
        if (owner == null) return false;
        return owner.Equals(System.Security.Principal.WindowsIdentity.GetCurrent().User)
            || owner.IsWellKnown(System.Security.Principal.WellKnownSidType.BuiltinAdministratorsSid);
    }

    public static bool Call(string json, out string reply, out string error)
    {
        reply = null;
        error = null;
        try
        {
            using (var p = new NamedPipeClientStream(".", PipeName(), PipeDirection.InOut))
            {
                p.Connect(1000);
                if (!OwnerTrusted(p)) { error = "pipe owner not trusted"; return false; }
                p.ReadMode = PipeTransmissionMode.Message;
                var req = Encoding.UTF8.GetBytes(json);
                p.Write(req, 0, req.Length);
                var buf = new byte[4096];
                var ms = new MemoryStream();
                do
                {
                    int n = p.Read(buf, 0, buf.Length);
                    if (n <= 0) break;
                    ms.Write(buf, 0, n);
                } while (!p.IsMessageComplete);
                reply = Encoding.UTF8.GetString(ms.ToArray());
                return true;
            }
        }
        catch (Exception ex)
        {
            error = ex.Message;
            return false;
        }
    }
}
