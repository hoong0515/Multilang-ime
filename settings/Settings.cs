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

static class Program
{
    [DllImport("user32.dll")]
    static extern bool SetProcessDPIAware();

    [STAThread]
    static void Main()
    {
        try { SetProcessDPIAware(); } catch { }
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new SettingsForm());
    }
}

class SettingsForm : Form
{
    static readonly string Dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "multilang-ime");
    static readonly string FilePath = Path.Combine(Dir, "settings.json");

    Dictionary<string, object> data = new Dictionary<string, object>();
    readonly CheckBox ko = new CheckBox { Text = "한국어", AutoSize = true };
    readonly CheckBox ja = new CheckBox { Text = "日本語 (일본어)", AutoSize = true };
    readonly CheckBox en = new CheckBox { Text = "English (항상 사용)", AutoSize = true, Checked = true, Enabled = false };
    readonly RadioButton spaceCommit = new RadioButton { Text = "확정 + 전각 공백 (기본)", AutoSize = true };
    readonly RadioButton spaceContinue = new RadioButton { Text = "확정하지 않고 이어서 변환", AutoSize = true };
    readonly ListBox apps = new ListBox { Height = 110, IntegralHeight = false };
    readonly TextBox appName = new TextBox();

    public SettingsForm()
    {
        Text = "Multilang IME 설정";
        Font = new Font("Segoe UI", 9f);
        AutoScaleMode = AutoScaleMode.Font;
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        StartPosition = FormStartPosition.CenterScreen;
        AutoSize = true;
        AutoSizeMode = AutoSizeMode.GrowAndShrink;
        Padding = new Padding(12);

        var root = new FlowLayoutPanel { FlowDirection = FlowDirection.TopDown, AutoSize = true, WrapContents = false };
        Controls.Add(root);

        root.Controls.Add(Group("사용할 언어 (자동 판정·후보)", Flow(en, ko, ja)));
        root.Controls.Add(Group("일본어 문맥에서 Space", Flow(spaceCommit, spaceContinue)));

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

        var save = new Button { Text = "저장", AutoSize = true, MinimumSize = new Size(80, 0) };
        var close = new Button { Text = "닫기", AutoSize = true, MinimumSize = new Size(80, 0), DialogResult = DialogResult.Cancel };
        save.Click += (s, e) => { if (Save()) Close(); };
        close.Click += (s, e) => Close();
        AcceptButton = save;
        CancelButton = close;
        var path = new Label { Text = FilePath, AutoSize = true, ForeColor = SystemColors.GrayText };
        root.Controls.Add(path);
        root.Controls.Add(Row(save, close));

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
        var list = Get(data, "disabled_apps") as IEnumerable;
        if (list != null)
            foreach (var o in list)
                if (o is string) apps.Items.Add(o);
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
        var list = new List<string>();
        foreach (var o in apps.Items) list.Add((string)o);
        data["disabled_apps"] = list;
        try
        {
            Directory.CreateDirectory(Dir);
            var json = new JavaScriptSerializer().Serialize(data);
            var tmp = FilePath + ".tmp";
            File.WriteAllText(tmp, json, new UTF8Encoding(false));
            if (File.Exists(FilePath)) File.Replace(tmp, FilePath, null);
            else File.Move(tmp, FilePath);
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
        string err = null;
        if (Backend.Call("{\"op\":\"clear_learning\"}", out err))
        {
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
    // ipc::pipe_name 과 같은 규칙: 사용자 이름의 영숫자 외 글자는 '_'
    static string PipeName()
    {
        var sb = new StringBuilder("multilang-ime.");
        foreach (var c in Environment.UserName)
            sb.Append(c < 128 && char.IsLetterOrDigit(c) ? c : '_');
        return sb.ToString();
    }

    public static bool Call(string json, out string error)
    {
        error = null;
        try
        {
            using (var p = new NamedPipeClientStream(".", PipeName(), PipeDirection.InOut))
            {
                p.Connect(1000);
                p.ReadMode = PipeTransmissionMode.Message;
                var req = Encoding.UTF8.GetBytes(json);
                p.Write(req, 0, req.Length);
                var buf = new byte[4096];
                do { p.Read(buf, 0, buf.Length); } while (!p.IsMessageComplete);
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
