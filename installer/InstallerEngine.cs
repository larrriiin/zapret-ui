using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using Microsoft.Win32;

namespace ZapretSetup
{
    internal sealed class ExistingInstall
    {
        public string Path;
        public string Version;
        public bool AllUsers;
    }

    internal static class InstallerEngine
    {
        internal const string UninstallKey = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\ZAPRET";

        internal static ExistingInstall FindInstall(bool allUsers)
        {
            foreach (RegistryView view in new[] { RegistryView.Registry64, RegistryView.Registry32 })
            using (RegistryKey root = RegistryKey.OpenBaseKey(allUsers ? RegistryHive.LocalMachine : RegistryHive.CurrentUser, view))
            using (RegistryKey key = root.OpenSubKey(UninstallKey))
            {
                if (key == null) continue;
                string path = (key.GetValue("InstallLocation") as string ?? "").Trim('"');
                if (path.Length == 0) continue;
                return new ExistingInstall { Path = path, Version = key.GetValue("DisplayVersion") as string ?? "", AllUsers = allUsers };
            }
            return null;
        }

        internal static string DefaultPath(bool allUsers)
        {
            return System.IO.Path.Combine(Environment.GetFolderPath(allUsers ? Environment.SpecialFolder.ProgramFiles : Environment.SpecialFolder.LocalApplicationData), "ZAPRET");
        }

        internal static string ValidatePath(string input)
        {
            if (String.IsNullOrWhiteSpace(input) || input.IndexOfAny(new[] { '"', '\r', '\n', '\t' }) >= 0)
                throw new ArgumentException("Укажите корректную папку установки.");
            if (input.Length < 4 || !Char.IsLetter(input[0]) || input[1] != ':' || input[2] != '\\')
                throw new ArgumentException("Выберите папку на локальном диске, например C:\\Apps\\ZAPRET.");
            if (input.IndexOf(':', 2) >= 0 || input.IndexOfAny(new[] { '<', '>', '|', '*', '?' }) >= 0)
                throw new ArgumentException("Путь содержит недопустимые символы.");
            foreach (string part in input.Substring(3).Replace('/', '\\').Split('\\'))
                if (part.EndsWith(".") || part.EndsWith(" "))
                    throw new ArgumentException("Названия папок не должны заканчиваться точкой или пробелом.");
            string path = System.IO.Path.GetFullPath(input.Trim()).TrimEnd('\\');
            if (path.Length > 180 || path.EndsWith(".") || path.EndsWith(" ") || path.IndexOf(':', 2) >= 0)
                throw new ArgumentException("Выберите более короткий путь без специальных символов в конце.");
            string windows = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
            if (path.Length <= 3 || path.Equals(windows, StringComparison.OrdinalIgnoreCase) || path.StartsWith(windows + "\\", StringComparison.OrdinalIgnoreCase))
                throw new ArgumentException("Выберите отдельную папку для ZAPRET вне папки Windows.");
            foreach (Environment.SpecialFolder folder in new[] { Environment.SpecialFolder.ProgramFiles, Environment.SpecialFolder.ProgramFilesX86, Environment.SpecialFolder.UserProfile, Environment.SpecialFolder.LocalApplicationData, Environment.SpecialFolder.DesktopDirectory, Environment.SpecialFolder.MyDocuments })
                if (path.Equals(Environment.GetFolderPath(folder), StringComparison.OrdinalIgnoreCase))
                    throw new ArgumentException("Создайте отдельную папку ZAPRET внутри выбранной папки.");
            if (File.Exists(path)) throw new ArgumentException("По этому пути уже существует файл. Выберите папку.");
            return path;
        }

        // NSIS requires /D to be the final argument and unquoted, including paths with spaces.
        internal static string Arguments(string path, bool allUsers)
        {
            return "/S /" + (allUsers ? "allusers" : "currentuser") + " /D=" + ValidatePath(path);
        }

        internal static bool PathsOverlap(string first, string second)
        {
            string a = Path.GetFullPath(first).TrimEnd('\\') + "\\";
            string b = Path.GetFullPath(second).TrimEnd('\\') + "\\";
            return a.StartsWith(b, StringComparison.OrdinalIgnoreCase) || b.StartsWith(a, StringComparison.OrdinalIgnoreCase);
        }

        internal static bool IsAppRunning()
        {
            foreach (Process process in Process.GetProcessesByName("zapret-ui"))
                using (process) { return true; }
            return false;
        }

        internal static int Install(string path, bool allUsers, Action<string> progress)
        {
            return RunPayload(Arguments(path, allUsers), allUsers, progress);
        }

        internal static int Update(UpdateOptions options, Action<string> progress)
        {
            if (options == null) throw new ArgumentNullException("options");
            // Let the same NSIS logic as previous releases restore install scope/path and restart
            // through RunAsUser. Never invent /D or choose a different user's registry hive here.
            return RunPayload(options.PayloadArguments, false, progress);
        }

        private static int RunPayload(string arguments, bool allUsers, Action<string> progress)
        {
            string directory = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "ZAPRET-Setup-" + Guid.NewGuid().ToString("N"));
            string payload = System.IO.Path.Combine(directory, "install.exe");
            // Keep the extracted executable private while still allowing an elevated installer to read it.
            DirectorySecurity security = new DirectorySecurity();
            security.SetAccessRuleProtection(true, false);
            foreach (SecurityIdentifier sid in new[] { WindowsIdentity.GetCurrent().User, new SecurityIdentifier(WellKnownSidType.BuiltinAdministratorsSid, null), new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null) })
                security.AddAccessRule(new FileSystemAccessRule(sid, FileSystemRights.FullControl, InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit, PropagationFlags.None, AccessControlType.Allow));
            Directory.CreateDirectory(directory, security);
            try
            {
                using (Stream source = Assembly.GetExecutingAssembly().GetManifestResourceStream("payload.exe"))
                using (FileStream destination = new FileStream(payload, FileMode.CreateNew, FileAccess.Write, FileShare.None))
                {
                    if (source == null) throw new InvalidDataException("В установщике отсутствует пакет приложения.");
                    source.CopyTo(destination);
                }
                using (SHA256 sha = SHA256.Create())
                using (FileStream file = File.OpenRead(payload))
                    if (!BitConverter.ToString(sha.ComputeHash(file)).Replace("-", "").Equals(BuildInfo.PayloadHash, StringComparison.OrdinalIgnoreCase))
                        throw new InvalidDataException("Пакет установки повреждён. Скачайте установщик заново.");
                progress(allUsers ? "Подтвердите запрос Windows. Затем начнётся установка файлов и компонентов." : "Устанавливаем файлы и компоненты. При необходимости будет загружен WebView2.");
                // The parent stays unelevated so launching the app never inherits administrator rights.
                ProcessStartInfo start = new ProcessStartInfo(payload, arguments) { UseShellExecute = true, WorkingDirectory = directory };
                if (allUsers) start.Verb = "runas";
                using (Process child = Process.Start(start))
                {
                    if (child == null) throw new IOException("Не удалось запустить установку.");
                    child.WaitForExit();
                    return child.ExitCode;
                }
            }
            finally
            {
                // Delete only files we created, never traverse an installation or user directory.
                try { File.Delete(payload); Directory.Delete(directory, false); }
                catch (IOException) { }
                catch (UnauthorizedAccessException) { }
            }
        }
    }
}
