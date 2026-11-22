// 자동 꺼짐 시험용 창: 일반 / 비밀번호 / 숫자(InputScope) / 이메일(InputScope) 입력칸.
// 빌드: powershell -File tools\build-scope-test.ps1
using System;
using System.Runtime.InteropServices;
using System.Windows.Forms;

static class ImeScopeTest
{
    [DllImport("msctf.dll")]
    static extern int SetInputScope(IntPtr hwnd, int inputScope);
    [DllImport("user32.dll")]
    static extern int GetWindowLong(IntPtr hwnd, int index);
    [DllImport("user32.dll")]
    static extern int SetWindowLong(IntPtr hwnd, int index, int value);
    const int GWL_STYLE = -16, ES_NUMBER = 0x2000;

    const int IS_EMAIL_SMTPEMAILADDRESS = 5;
    const int IS_NUMBER = 29;

    [STAThread]
    static void Main()
    {
        Application.EnableVisualStyles();
        var form = new Form { Text = "IME Scope Test", Width = 520, Height = 300 };
        string[] labels = { "Normal", "Password", "Number (IS_NUMBER)", "Email (IS_EMAIL)" };
        for (int i = 0; i < labels.Length; i++)
        {
            form.Controls.Add(new Label { Text = labels[i], Left = 12, Top = 20 + i * 50, Width = 160 });
            var tb = new TextBox { Name = "tb" + i, Left = 180, Top = 16 + i * 50, Width = 300 };
            if (i == 1) tb.UseSystemPasswordChar = true;
            int scope = i == 2 ? IS_NUMBER : i == 3 ? IS_EMAIL_SMTPEMAILADDRESS : 0;
            if (scope != 0) tb.HandleCreated += (s, e) => SetInputScope(((TextBox)s).Handle, scope);
            // 숫자 칸은 옛 방식의 숫자 전용 스타일도 건다 (CUAS 경유 앱은 InputScope를 TIP가 읽지 못한다)
            if (i == 2) tb.HandleCreated += (s, e) => { var h = ((TextBox)s).Handle; SetWindowLong(h, GWL_STYLE, GetWindowLong(h, GWL_STYLE) | ES_NUMBER); };
            form.Controls.Add(tb);
        }
        Application.Run(form);
    }
}
