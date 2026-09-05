using System;
using System.Text.RegularExpressions;

namespace ZapretSetup
{
    internal sealed class UpdateOptions
    {
        internal bool Quiet;
        internal string PayloadArguments;

        internal static string WithoutExecutable(string commandLine)
        {
            string command = commandLine.TrimStart();
            int end = command.StartsWith("\"") ? command.IndexOf('"', 1) + 1 : command.IndexOf(' ');
            return end <= 0 ? "" : command.Substring(end).TrimStart();
        }

        internal static UpdateOptions Parse(string commandLine)
        {
            // /ARGS belongs to the restarted application. Preserve its raw NSIS-escaped bytes:
            // re-quoting the CLR args would corrupt spaces, quotes and escaped slash arguments.
            Match boundary = Regex.Match(commandLine, @"(?:^|\s)/ARGS(?:\s|$)", RegexOptions.IgnoreCase);
            string prefix = boundary.Success ? commandLine.Substring(0, boundary.Index) : commandLine;
            string tail = boundary.Success ? commandLine.Substring(boundary.Index).TrimStart() : "";
            bool update = false, quiet = false, restart = false;
            foreach (string token in prefix.Split((char[])null, StringSplitOptions.RemoveEmptyEntries))
            {
                if (token.Equals("/UPDATE", StringComparison.OrdinalIgnoreCase)) update = true;
                else if (token.Equals("/S", StringComparison.OrdinalIgnoreCase)) quiet = true;
                else if (token.Equals("/R", StringComparison.OrdinalIgnoreCase)) restart = true;
                else if (!token.Equals("/P", StringComparison.OrdinalIgnoreCase)) return null;
            }
            if (!update) return null;
            return new UpdateOptions {
                Quiet = quiet,
                PayloadArguments = "/S /UPDATE" + (restart ? " /R" : "") + (tail.Length > 0 ? " " + tail : "")
            };
        }
    }
}
