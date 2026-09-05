using System;
using System.IO;
internal static class FakePayload
{
    private static int Main(string[] args)
    {
        // A harmless child process: record the command line; never touch registry, shortcuts or installed apps.
        File.WriteAllText(Environment.GetEnvironmentVariable("ZAPRET_TEST_RESULT"), Environment.CommandLine);
        return Int32.Parse(Environment.GetEnvironmentVariable("ZAPRET_TEST_EXIT") ?? "0");
    }
}
