using System;
using System.IO;
using ZapretSetup;
internal static class IntegrityTests
{
    private static int Main(string[] args)
    {
        string marker = Path.Combine(args[0], "must-not-exist.txt");
        Environment.SetEnvironmentVariable("ZAPRET_TEST_RESULT", marker);
        try { InstallerEngine.Install(Path.Combine(args[0], "ZAPRET"), false, text => { }); }
        catch (InvalidDataException)
        {
            if (File.Exists(marker)) throw new Exception("Corrupt payload was executed.");
            Console.WriteLine("PASS: corrupt payload is rejected before execution.");
            return 0;
        }
        throw new Exception("Payload integrity check did not reject the incorrect hash.");
    }
}
