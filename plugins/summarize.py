"""Potter Plugin — /summarize

Summarizes the given text in a concise, bullet-point format.

Usage:
    /summarize <text or paste long content>

Example:
    /summarize <paste a long article here>
"""

COMMAND = "/summarize"
DESCRIPTION = "Summarize text into concise bullet points"
VERSION = "1.0.0"


def run(args: str) -> str:
    """
    Wraps the user's text in a summarization prompt.

    :param args: The text to summarize
    :return: A prompt string for the LLM
    """
    text = args.strip()
    if not text:
        return "Please provide text to summarize."

    return (
        "Summarize the following text in 3–5 concise bullet points. "
        "Be direct and omit filler phrases.\n\n"
        f"{text}"
    )
