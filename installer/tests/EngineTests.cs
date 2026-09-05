using System;
using System.IO;
using ZapretSetup;

internal static class EngineTests
{
    private static int checks;
    private static void Check(bool value, string name) { if (!value) throw new Exception(name); checks++; }
    private static int Main(string[] args)
    {
        string root = args[0];
        string destination = Path.Combine(root, "Папка с пробелами", "ZAPRET");
        Check(InstallerEngine.Arguments(destination, false) == "/S /currentuser /D=" + destination, "Current-user arguments preserve spaces and Cyrillic");
        Check(InstallerEngine.Arguments(destination, true) == "/S /allusers /D=" + destination, "Machine arguments put /D last");
        Check(InstallerEngine.PathsOverlap(destination, destination.ToUpperInvariant()), "Scope paths compare case-insensitively");
        Check(InstallerEngine.PathsOverlap(destination, Path.Combine(destination, "child")), "Nested scope paths conflict");
        Check(InstallerEngine.PathsOverlap(Path.Combine(destination, "child"), destination), "Parent scope paths conflict");
        Check(!InstallerEngine.PathsOverlap(destination, destination + "-other"), "Sibling scope paths are distinct");
        foreach (string invalid in new[] { "", "relative", @"C:\", @"\\server\share\ZAPRET", "C:\\ZAPRET\" /R", "C:\\ZAPRET\n/R", @"C:\ZAPRET:stream", Environment.GetFolderPath(Environment.SpecialFolder.Windows), Environment.GetFolderPath(Environment.SpecialFolder.UserProfile) })
        {
            bool rejected = false;
            try { InstallerEngine.ValidatePath(invalid); } catch (ArgumentException) { rejected = true; }
            Check(rejected, "Reject unsafe path: " + invalid);
        }
        string result = Path.Combine(root, "child-arguments.txt");
        Environment.SetEnvironmentVariable("ZAPRET_TEST_RESULT", result);
        foreach (int expected in new[] { 0, 1, 3010 })
        {
            Environment.SetEnvironmentVariable("ZAPRET_TEST_EXIT", expected.ToString());
            bool progress = false;
            int actual = InstallerEngine.Install(destination, false, message => progress = true);
            Check(actual == expected, "Child exit code " + expected + " is preserved");
            Check(progress, "Progress callback occurs");
            string command = File.ReadAllText(result);
            Check(command.EndsWith("/S /currentuser /D=" + destination), "Child receives the complete NSIS command line");
            string executable = command.StartsWith("\"") ? command.Split('"')[1] : command.Substring(0, command.IndexOf(" /S"));
            Check(!File.Exists(executable), "Extracted payload is cleaned up");
        }
        Check(!Directory.Exists(destination), "Harness did not install an application");
        Check(UpdateOptions.WithoutExecutable("\"C:\\Program Files\\setup.exe\" /P /R /UPDATE /ARGS") == "/P /R /UPDATE /ARGS", "Quoted wrapper executable is removed without touching updater flags");
        Check(UpdateOptions.WithoutExecutable("C:\\setup.exe /UPDATE") == "/UPDATE", "Unquoted executable path is supported");
        Check(UpdateOptions.Parse("") == null, "Manual launch remains interactive");
        Check(UpdateOptions.Parse("/P /R") == null, "No update mode without /UPDATE");
        Check(UpdateOptions.Parse("/UPDATE /UNEXPECTED") == null, "Unsupported installer flags fail closed");
        string appArguments = " /ARGS \"hello world\" %2Fbackground \"C:\\Папка с пробелами\"";
        foreach (string flags in new[] { "/P /R /UPDATE", "/S /R /UPDATE", "/UPDATE" })
        {
            UpdateOptions options = UpdateOptions.Parse(flags + appArguments);
            Check(options != null, "Tauri updater protocol is accepted: " + flags);
            Check(options.Quiet == flags.StartsWith("/S"), "Quiet and visible passive modes are distinguished");
            string expected = "/S /UPDATE" + (flags.Contains("/R") ? " /R" : "") + appArguments;
            Check(options.PayloadArguments == expected, "Raw restarted-application arguments are preserved");
            Environment.SetEnvironmentVariable("ZAPRET_TEST_EXIT", "0");
            Check(InstallerEngine.Update(options, text => { }) == 0, "Updater payload completes");
            Check(File.ReadAllText(result).EndsWith(expected), "Inner installer receives unattended update and restart flags");
        }
        Console.WriteLine("PASS: " + checks + " installer checks (safe fake payload; no app installation).");
        return 0;
    }
}
