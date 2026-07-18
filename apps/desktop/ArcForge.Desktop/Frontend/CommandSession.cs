#nullable enable

using System;
using System.Collections.Generic;

namespace ArcForge.Desktop.Frontend;

public enum CommandOutcome
{
    Completed,
    Cancelled,
    Error,
}

public record CommandHistoryEntry(
    int Sequence,
    string Command,
    CommandOutcome Outcome,
    string Message);

public sealed class CommandInputException : Exception
{
    public CommandInputException(string code, string message)
        : base(message)
    {
        Code = code;
    }

    public string Code { get; }
}

public sealed class CommandSession
{
    private readonly List<CommandHistoryEntry> _history = new();
    private IReadOnlyList<string> _options = Array.Empty<string>();
    private int _historyCursor;
    private int _sequence;
    private bool _hasCommittedProgress;

    public string? ActiveCommand { get; private set; }

    public string Prompt { get; private set; } = string.Empty;

    public IReadOnlyList<string> Options => _options;

    public string? DefaultOption { get; private set; }

    public string? Preview { get; private set; }

    public string? LastCompletedCommand { get; private set; }

    public CommandInputException? LastError { get; private set; }

    public IReadOnlyList<CommandHistoryEntry> History => _history;

    public bool IsActive => ActiveCommand is not null;

    public void Begin(
        string command,
        string? prompt,
        IEnumerable<string>? options = null,
        string? defaultOption = null,
        string? preview = null)
    {
        if (IsActive)
        {
            throw Error("ACTIVE_COMMAND", $"Command {ActiveCommand} is already active.");
        }

        var normalizedCommand = NormalizeRequired(command, "INVALID_COMMAND", "Command cannot be blank.");
        var normalizedOptions = new List<string>();
        var seen = new HashSet<string>(StringComparer.Ordinal);
        if (options is not null)
        {
            foreach (var option in options)
            {
                var normalizedOption = NormalizeRequired(
                    option,
                    "INVALID_OPTION",
                    "Command options cannot be blank.");
                if (!seen.Add(normalizedOption))
                {
                    throw Error("DUPLICATE_OPTION", $"Duplicate command option: {normalizedOption}.");
                }

                normalizedOptions.Add(normalizedOption);
            }
        }

        string? normalizedDefault = null;
        if (defaultOption is not null)
        {
            normalizedDefault = NormalizeRequired(
                defaultOption,
                "INVALID_DEFAULT",
                "Default option cannot be blank.");
            if (!seen.Contains(normalizedDefault))
            {
                throw Error("INVALID_DEFAULT", $"Default option is not listed: {normalizedDefault}.");
            }
        }

        ActiveCommand = normalizedCommand;
        Prompt = prompt ?? string.Empty;
        _options = normalizedOptions.ToArray();
        DefaultOption = normalizedDefault;
        Preview = preview;
        LastError = null;
        _hasCommittedProgress = false;
        _historyCursor = _history.Count;
    }

    public string ResolveInput(string? input)
    {
        var normalized = Normalize(input);
        if (!IsActive)
        {
            if (normalized.Length == 0)
            {
                if (LastCompletedCommand is null)
                {
                    throw Error("NO_REPEAT", "There is no completed command to repeat.");
                }

                LastError = null;
                return LastCompletedCommand;
            }

            LastError = null;
            return normalized;
        }

        if (normalized.Length == 0)
        {
            if (DefaultOption is null)
            {
                throw Error("MISSING_INPUT", "The active command requires input.");
            }

            LastError = null;
            return DefaultOption;
        }

        return ResolveOption(normalized);
    }

    public string ResolveOption(string? option)
    {
        RequireActive();
        var normalized = Normalize(option);
        string? prefixMatch = null;
        foreach (var candidate in _options)
        {
            if (candidate == normalized)
            {
                LastError = null;
                return candidate;
            }

            if (normalized.Length > 0 && candidate.StartsWith(normalized, StringComparison.Ordinal))
            {
                if (prefixMatch is not null)
                {
                    throw Error("AMBIGUOUS_OPTION", $"Ambiguous option for {ActiveCommand}: {normalized}.");
                }

                prefixMatch = candidate;
            }
        }

        if (prefixMatch is not null)
        {
            LastError = null;
            return prefixMatch;
        }

        throw Error("INVALID_OPTION", $"Invalid option for {ActiveCommand}: {normalized}.");
    }

    public void SetPrompt(string? prompt)
    {
        Prompt = prompt ?? string.Empty;
    }

    public void SetPreview(string? preview)
    {
        RequireActive();
        Preview = preview;
    }

    public void MarkProgress(string? message = null)
    {
        RequireActive();
        _hasCommittedProgress = true;
        if (message is not null)
        {
            Prompt = message;
        }
    }

    public void Complete(string? message = null)
    {
        RequireActive();
        var command = ActiveCommand!;
        LastCompletedCommand = command;
        LastError = null;
        AddHistory(command, CommandOutcome.Completed, string.Empty);
        ClearActiveState();
        Prompt = message ?? string.Empty;
    }

    public bool Cancel(string? message = null)
    {
        if (!IsActive)
        {
            return false;
        }

        var command = ActiveCommand!;
        var outcome = _hasCommittedProgress
            ? CommandOutcome.Completed
            : CommandOutcome.Cancelled;
        if (outcome == CommandOutcome.Completed)
        {
            LastCompletedCommand = command;
        }

        LastError = null;
        AddHistory(command, outcome, message ?? string.Empty);
        ClearActiveState();
        Prompt = message ?? string.Empty;
        return true;
    }

    public void Fail(string command, string message)
    {
        var normalizedCommand = NormalizeRequired(
            command,
            "INVALID_COMMAND",
            "Failed command cannot be blank.");
        var error = new CommandInputException("COMMAND_ERROR", message ?? string.Empty);
        AddHistory(normalizedCommand, CommandOutcome.Error, error.Message);
        ClearActiveState();
        LastError = error;
    }

    public void RejectInput(string message)
    {
        RequireActive();
        var error = new CommandInputException("INVALID_INPUT", message ?? string.Empty);
        LastError = error;
        AddHistory(ActiveCommand!, CommandOutcome.Error, error.Message);
    }

    public string? PreviousHistoryCommand()
    {
        if (_history.Count == 0)
        {
            return null;
        }

        if (_historyCursor > 0)
        {
            _historyCursor--;
        }

        return _history[_historyCursor].Command;
    }

    public string? NextHistoryCommand()
    {
        if (_history.Count == 0 || _historyCursor >= _history.Count - 1)
        {
            _historyCursor = _history.Count;
            return null;
        }

        _historyCursor++;
        return _history[_historyCursor].Command;
    }

    private static string Normalize(string? value) =>
        value?.Trim().ToUpperInvariant() ?? string.Empty;

    private string NormalizeRequired(string? value, string code, string message)
    {
        var normalized = Normalize(value);
        if (normalized.Length == 0)
        {
            throw Error(code, message);
        }

        return normalized;
    }

    private void RequireActive()
    {
        if (!IsActive)
        {
            throw Error("NO_ACTIVE_COMMAND", "There is no active command.");
        }
    }

    private CommandInputException Error(string code, string message)
    {
        var error = new CommandInputException(code, message);
        LastError = error;
        return error;
    }

    private void AddHistory(string command, CommandOutcome outcome, string message)
    {
        _history.Add(new CommandHistoryEntry(++_sequence, command, outcome, message));
        _historyCursor = _history.Count;
    }

    private void ClearActiveState()
    {
        ActiveCommand = null;
        Prompt = string.Empty;
        _options = Array.Empty<string>();
        DefaultOption = null;
        Preview = null;
        _hasCommittedProgress = false;
    }
}
