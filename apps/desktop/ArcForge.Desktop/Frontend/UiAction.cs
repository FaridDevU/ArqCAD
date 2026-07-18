namespace ArcForge.Desktop.Frontend;

public enum UiActionKind
{
    Immediate,
    Toggle,
    Flyout,
    Dock,
    Modal,
    Navigate,
    Unavailable,
}

public sealed record UiAction(
    string Id,
    string Label,
    UiActionKind Kind,
    string Response,
    string? Group = null,
    bool IsBackendDependent = false)
{
    public void Validate()
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(Id);
        ArgumentException.ThrowIfNullOrWhiteSpace(Label);
        ArgumentException.ThrowIfNullOrWhiteSpace(Response);
    }
}
