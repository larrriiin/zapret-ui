using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Security.Principal;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Markup;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Forms = System.Windows.Forms;

namespace ZapretSetup
{
    internal static class Program
    {
        [STAThread]
        private static int Main(string[] args)
        {
            bool render = args.Length == 2 && args[0] == "--render";
            bool preview = args.Length == 1 && args[0] == "--preview";
            UpdateOptions update = render || preview ? null : UpdateOptions.Parse(UpdateOptions.WithoutExecutable(Environment.CommandLine));
            if (args.Length > 0 && !render && !preview && update == null) return 2;
            bool first;
            using (Mutex mutex = new Mutex(true, @"Local\ZAPRET-Branded-Setup", out first))
            {
                if (!first && !render) { MessageBox.Show("Установщик ZAPRET уже открыт.", "ZAPRET"); return 1; }
                try
                {
                    Application app = new Application();
                    SetupWindow setup = new SetupWindow(preview || render || BuildInfo.PreviewOnly, update);
                    if (render) { setup.Render(args[1]); return 0; }
                    app.Run(setup.Window);
                    return setup.ExitCode;
                }
                catch (Exception error)
                {
                    if (render) { File.WriteAllText(Path.Combine(args[1], "render-error.txt"), error.ToString()); return 1; }
                    MessageBox.Show("Не удалось открыть установщик.\n\n" + error.Message, "ZAPRET", MessageBoxButton.OK, MessageBoxImage.Error);
                    return 1;
                }
            }
        }
    }

    internal sealed class SetupWindow
    {
        internal Window Window;
        private readonly bool preview;
        private bool busy;
        private string state = "setup";
        private string installedPath;
        private bool reboot;
        private bool elevated;
        private ExistingInstall userInstall;
        private ExistingInstall machineInstall;
        private readonly UpdateOptions update;
        private bool updateStarted;
        internal int ExitCode;
        private T Find<T>(string name) where T : class { return Window.FindName(name) as T; }
        private void Text(string name, string value) { Find<TextBlock>(name).Text = value; }

        internal SetupWindow(bool preview, UpdateOptions update = null)
        {
            this.preview = preview;
            this.update = update;
            using (Stream stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("Installer.xaml"))
                Window = (Window)XamlReader.Load(stream);
            if (!preview)
            {
                Window.Width = Math.Min(Window.Width, Math.Max(480, SystemParameters.WorkArea.Width - 16));
                Window.Height = Math.Min(Window.Height, Math.Max(320, SystemParameters.WorkArea.Height - 16));
            }
            using (WindowsIdentity identity = WindowsIdentity.GetCurrent())
                elevated = new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator);
            using (Stream stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("icon.png"))
            {
                BitmapImage icon = new BitmapImage();
                icon.BeginInit(); icon.CacheOption = BitmapCacheOption.OnLoad; icon.StreamSource = stream; icon.EndInit(); icon.Freeze();
                Window.Icon = icon; Find<Image>("BrandIcon").Source = icon;
            }
            Text("VersionLabel", BuildInfo.Version + "  ·  x64");
            if (!preview)
            {
                userInstall = InstallerEngine.FindInstall(false);
                machineInstall = InstallerEngine.FindInstall(true);
            }
            Find<RadioButton>("AllUsers").IsChecked = userInstall == null && machineInstall != null;
            Find<RadioButton>("CurrentUser").IsChecked = !Find<RadioButton>("AllUsers").IsChecked.Value;
            Find<RadioButton>("CurrentUser").Checked += delegate { RefreshPath(); };
            Find<RadioButton>("AllUsers").Checked += delegate { RefreshPath(); };
            Find<Button>("BrowseButton").Click += delegate { Browse(); };
            Find<Button>("CloseButton").Click += delegate { Window.Close(); };
            Find<Button>("SecondaryButton").Click += delegate { Window.Close(); };
            Find<Button>("MinimizeButton").Click += delegate { Window.WindowState = WindowState.Minimized; };
            Find<Button>("PrimaryButton").Click += async delegate { await Primary(); };
            Find<System.Windows.Controls.Border>("TitleBar").MouseLeftButtonDown += delegate(object sender, MouseButtonEventArgs e)
            {
                if (e.OriginalSource is TextBlock || e.OriginalSource is Border || e.OriginalSource is Image) Window.DragMove();
            };
            Window.Closing += delegate(object sender, CancelEventArgs e) { e.Cancel = busy; };
            RefreshPath();
            if (preview) Text("FooterNote", "Предпросмотр — приложение не устанавливается.");
            if (update != null)
            {
                Show("progress");
                Text("ProgressHeading", "Обновляем ZAPRET");
                Text("ProgressDescription", "После обновления приложение запустится автоматически.");
                if (update.Quiet) { Window.Opacity = 0; Window.ShowInTaskbar = false; }
                Window.ContentRendered += async delegate { await RunUpdate(); };
            }
        }

        private async Task RunUpdate()
        {
            if (updateStarted) return;
            updateStarted = true;
            try
            {
                int exit;
                if (preview) { await Task.Delay(1800); exit = 0; }
                else exit = await Task.Run(() => InstallerEngine.Update(update, message => Window.Dispatcher.Invoke(new Action(() => Text("ProgressText", message)))));
                ExitCode = exit;
                if (exit != 0 && exit != 3010)
                { Error("Не удалось завершить обновление. Откройте ZAPRET и попробуйте обновить его ещё раз.", "Код установщика: " + exit); return; }
                // /R is handled by the inner NSIS, including de-elevation and original app arguments.
                // Do not show a success wizard or launch a second application instance.
                busy = false;
                Window.Close();
            }
            catch (Win32Exception e)
            { ExitCode = e.NativeErrorCode; Error(e.NativeErrorCode == 1223 ? "Запрос Windows отменён. Обновление не выполнено." : "Windows не удалось запустить обновление.", e.Message); }
            catch (Exception e)
            { ExitCode = 1; Error("Не удалось завершить обновление. Откройте ZAPRET и повторите попытку.", e.Message); }
        }

        private void RefreshPath()
        {
            bool all = Find<RadioButton>("AllUsers").IsChecked == true;
            ExistingInstall existing = all ? machineInstall : userInstall;
            Find<TextBox>("InstallPath").Text = existing == null ? InstallerEngine.DefaultPath(all) : existing.Path;
            Find<TextBox>("InstallPath").IsReadOnly = existing != null;
            Find<Button>("BrowseButton").IsEnabled = existing == null;
            Text("Heading", existing == null ? "Установим ZAPRET" : "Обновим ZAPRET");
            Find<Button>("PrimaryButton").Content = existing == null ? "Установить" : "Обновить";
            Text("ScopeNote", existing != null
                ? "Найдена версия " + existing.Version + ". Используем её папку установки. Перед обновлением закройте ZAPRET, в том числе в трее."
                : all ? "Windows запросит права администратора. Приложение будет доступно всем пользователям компьютера."
                      : "Приложение будет доступно в вашей учётной записи Windows.");
        }

        private void Browse()
        {
            using (Forms.FolderBrowserDialog dialog = new Forms.FolderBrowserDialog())
            {
                dialog.Description = "Выберите отдельную папку для ZAPRET";
                dialog.SelectedPath = Find<TextBox>("InstallPath").Text;
                if (dialog.ShowDialog(new WindowOwner(Window)) == Forms.DialogResult.OK)
                    Find<TextBox>("InstallPath").Text = dialog.SelectedPath;
            }
        }

        private void Show(string next)
        {
            state = next;
            foreach (string panel in new[] { "Setup", "Progress", "Done", "Error" })
                Find<StackPanel>(panel + "Panel").Visibility = panel.Equals(next, StringComparison.OrdinalIgnoreCase) ? Visibility.Visible : Visibility.Collapsed;
            busy = next == "progress";
            Find<Button>("CloseButton").IsEnabled = !busy;
            Find<Button>("PrimaryButton").IsEnabled = !busy;
            Find<Button>("SecondaryButton").Visibility = next == "done" ? Visibility.Visible : Visibility.Collapsed;
            Find<Button>("PrimaryButton").Content = next == "done" ? (reboot || elevated ? "Закрыть" : "Запустить ZAPRET") : next == "error" ? "Назад" : next == "progress" ? "Установка…" : "Установить";
            if (next == "done" && (reboot || elevated)) Find<Button>("SecondaryButton").Visibility = Visibility.Collapsed;
            Text("FooterNote", preview ? "Предпросмотр — приложение не устанавливается." : busy ? "Не выключайте компьютер во время установки." : next == "done" ? "Спасибо, что выбираете ZAPRET." : "После установки поможем с первой настройкой.");
            for (int i = 1; i <= 3; i++)
                Find<TextBlock>("Step" + i).Foreground = new SolidColorBrush((Color)ColorConverter.ConvertFromString(i == (next == "progress" ? 2 : next == "done" ? 3 : 1) ? "#BA9EFF" : "#A5AAC2"));
            if (!busy) Find<Button>("PrimaryButton").Focus();
        }

        private void Error(string message, string detail)
        {
            Window.Opacity = 1;
            Window.ShowInTaskbar = true;
            Text("ErrorText", message);
            Find<TextBox>("ErrorDetail").Text = detail;
            Find<TextBox>("ErrorDetail").Visibility = String.IsNullOrEmpty(detail) ? Visibility.Collapsed : Visibility.Visible;
            Show("error");
            if (update != null) Find<Button>("PrimaryButton").Content = "Закрыть";
        }

        private async Task Primary()
        {
            if (busy) return;
            if (state == "error") { if (update != null) { Window.Close(); return; } Show("setup"); RefreshPath(); return; }
            if (state == "done")
            {
                if (preview || reboot || elevated) { Window.Close(); return; }
                try { Process.Start(new ProcessStartInfo(Path.Combine(installedPath, "zapret-ui.exe")) { UseShellExecute = true, WorkingDirectory = installedPath }); Window.Close(); }
                catch (Exception e) { Error("ZAPRET установлен, но запустить его не удалось. Откройте приложение через меню «Пуск».", e.Message); }
                return;
            }
            if (preview) { Show("progress"); await Task.Delay(1800); Show("done"); return; }
            try
            {
                bool all = Find<RadioButton>("AllUsers").IsChecked == true;
                string path = InstallerEngine.ValidatePath(Find<TextBox>("InstallPath").Text);
                if (InstallerEngine.IsAppRunning()) { Error("Закройте ZAPRET через значок в трее и повторите установку.", "Работающее приложение не будет закрыто автоматически оболочкой."); return; }
                // Recheck registry just before starting: a previous failed attempt may have registered the app.
                ExistingInstall existing = InstallerEngine.FindInstall(all);
                ExistingInstall other = InstallerEngine.FindInstall(!all);
                if (other != null && InstallerEngine.PathsOverlap(path, other.Path))
                { Error("Эта папка используется установкой для другого пользователя или для всего компьютера. Выберите соответствующий режим либо отдельную папку.", other.Path); return; }
                Version existingVersion;
                if (existing != null && Version.TryParse(existing.Version, out existingVersion) && existingVersion > new Version(BuildInfo.Version))
                { Error("На компьютере уже установлена более новая версия ZAPRET. Используйте актуальный установщик.", "Установлена: " + existing.Version + ". В пакете: " + BuildInfo.Version + "."); return; }
                if (existing != null && !Path.GetFullPath(existing.Path).TrimEnd('\\').Equals(path, StringComparison.OrdinalIgnoreCase))
                {
                    userInstall = InstallerEngine.FindInstall(false); machineInstall = InstallerEngine.FindInstall(true);
                    Error("Обнаружена другая папка установленного ZAPRET. Вернитесь назад, чтобы использовать её.", existing.Path); return;
                }
                Show("progress");
                int exit = await Task.Run(() => InstallerEngine.Install(path, all, message => Window.Dispatcher.Invoke(new Action(() => Text("ProgressText", message)))));
                if (exit != 0 && exit != 3010) { Error("Не удалось завершить установку. Проверьте свободное место и права доступа, затем повторите попытку.", "Код установщика: " + exit); return; }
                ExistingInstall result = InstallerEngine.FindInstall(all);
                if (result == null || result.Version != BuildInfo.Version || !File.Exists(Path.Combine(result.Path, "zapret-ui.exe")))
                { Error("Установщик завершился, но новая версия приложения не найдена. Повторите установку.", "Не удалось подтвердить версию " + BuildInfo.Version + "."); return; }
                installedPath = result.Path;
                reboot = exit == 3010;
                Text("DoneText", reboot ? "ZAPRET установлен. Перед запуском перезагрузите Windows." : elevated ? "ZAPRET установлен. Закройте установщик и откройте приложение через меню «Пуск»." : "ZAPRET установлен. Можно переходить к настройке.");
                Show("done");
            }
            catch (Win32Exception e)
            {
                Error(e.NativeErrorCode == 1223 ? "Запрос прав администратора отменён. Вернитесь назад и попробуйте ещё раз." : "Windows не удалось запустить установку.", e.NativeErrorCode == 1223 ? "" : e.Message);
            }
            catch (ArgumentException e) { Error(e.Message, ""); }
            catch (Exception e) { Error("Не удалось завершить установку. Вернитесь назад и попробуйте ещё раз.", e.Message); }
        }

        // Render the actual WPF visual tree without installing anything or opening a desktop window.
        internal void Render(string directory)
        {
            Directory.CreateDirectory(directory);
            GlyphTypeface glyph;
            if (!new Typeface(Window.FontFamily, FontStyles.Normal, FontWeights.Normal, FontStretches.Normal).TryGetGlyphTypeface(out glyph) || !glyph.FontUri.ToString().StartsWith("pack:"))
                throw new InvalidDataException("The embedded Inter font did not load.");
            File.WriteAllText(Path.Combine(directory, "render-checks.txt"), "Embedded font: " + glyph.FontUri + Environment.NewLine);
            foreach (string screenshot in new[] { "setup", "progress", "done", "error", "compact" })
            {
                string stage = screenshot == "compact" ? "setup" : screenshot;
                if (screenshot == "compact") { Window.Width = 640; Window.Height = 430; }
                if (stage == "error") Error("Запрос прав администратора отменён. Вернитесь назад и попробуйте ещё раз.", "");
                else Show(stage);
                if (stage == "setup") RefreshPath();
                FrameworkElement content = (FrameworkElement)Window.Content;
                content.Measure(new Size(Window.Width, Window.Height)); content.Arrange(new Rect(0, 0, Window.Width, Window.Height)); content.UpdateLayout();
                RenderTargetBitmap bitmap = new RenderTargetBitmap((int)Window.Width, (int)Window.Height, 96, 96, PixelFormats.Pbgra32);
                bitmap.Render(content);
                PngBitmapEncoder encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap));
                using (FileStream output = File.Create(Path.Combine(directory, screenshot + ".png"))) encoder.Save(output);
                Button primary = Find<Button>("PrimaryButton");
                Point position = primary.TranslatePoint(new Point(0, 0), content);
                if (position.Y + primary.ActualHeight > Window.Height || position.X + primary.ActualWidth > Window.Width)
                    throw new InvalidDataException("Primary button falls outside the window: " + screenshot);
                File.AppendAllText(Path.Combine(directory, "render-checks.txt"), screenshot + ": primary action fits " + Window.Width + "x" + Window.Height + Environment.NewLine);
            }
        }

        private sealed class WindowOwner : Forms.IWin32Window
        {
            private readonly Window window;
            public WindowOwner(Window window) { this.window = window; }
            public IntPtr Handle { get { return new System.Windows.Interop.WindowInteropHelper(window).Handle; } }
        }
    }
}
