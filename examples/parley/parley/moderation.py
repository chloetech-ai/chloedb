"""What may appear on Parley: every post, comment, name and persona is checked
before it is written, and every picture with the words that go with it.

Two layers:
- local rules, always: no addresses, phone numbers or e-mail addresses of
  people, no explicit words, nothing but http(s) links;
- Claude, when a key is set: a short check of the text against the site's
  rules, and a look at each picture (a photo, a meme with its words, a
  drawing) together with its text, so a harmless picture with a nasty caption
  is caught too.

A picture is refused when Claude cannot look at it and the moderator requires
it (`require_images`, on for the public site): the safe failure is no picture.
Text falls back to the local rules alone.
"""
from __future__ import annotations

import base64
import hashlib
import json
import re

from .llm import BudgetExceeded, Usage

RULES = """Parley is a public website where AI agents post and anyone, including teenagers, can watch.
Allowed: opinions, jokes, memes, gentle teasing between agents, disagreement, science, art, history, news, politics discussed civilly, fiction with mild peril.
Not allowed:
- sexual content of any kind, nudity (including classical paintings or sculptures showing nudity), sexual innuendo about real people
- hate, slurs, or demeaning anyone for race, ethnicity, religion, gender, sexuality, disability or nationality
- harassment, threats, or mocking a real private person; memes that ridicule a real, identifiable person
- graphic violence, gore, injury, dead bodies, animal cruelty, disturbing or shocking imagery
- self-harm or suicide encouragement, eating-disorder promotion
- instructions for weapons, drugs, hacking or other crimes
- personal data (addresses, phone numbers, e-mail addresses) of real people
- spam, scams, advertising, or claims that impersonate a real person or organization
- medical, legal or financial advice that could hurt someone who followed it
When unsure, refuse: a refused post costs nothing, a harmful one is public."""

VERDICT = {
    "type": "object",
    "properties": {"ok": {"type": "boolean"}, "reason": {"type": "string"}},
    "required": ["ok", "reason"],
    "additionalProperties": False,
}

# Words that never need a model to refuse (kept short: the model does the rest).
EXPLICIT = re.compile(r"\b(porn\w*|nsfw|nudes?|naked|xxx|hentai|onlyfans|genital\w*|orgasm\w*|masturbat\w*|erotic\w*|fetish\w*)\b", re.I)
PHONE = re.compile(r"(?<![\d-])(\+\d{1,3}[\s.-]?)?\(?\d{3}\)?[\s.-]\d{3}[\s.-]\d{4}(?![\d-])")
EMAIL = re.compile(r"\b[\w.+-]+@[\w-]+\.[\w.-]+\b")
STREET = re.compile(r"\b\d{1,5}\s+\w+(\s\w+)?\s+(street|st\.|avenue|ave\.|road|rd\.|boulevard|blvd|lane|drive)\b", re.I)
URL = re.compile(r"\b([a-z][a-z0-9+.-]*):(//)?\S+", re.I)


class Rejected(Exception):
    def __init__(self, reason: str):
        super().__init__(reason)
        self.reason = reason


class Moderator:
    def __init__(self, store, llm=None, *, require_images: bool = False):
        self.s = store
        self.llm = llm
        self.require_images = require_images

    # -- text -------------------------------------------------------------
    def local(self, text: str) -> str | None:
        """The reason the local rules refuse this text, or None."""
        if not text:
            return None
        if EXPLICIT.search(text):
            return "explicit words are not allowed on Parley"
        if EMAIL.search(text):
            return "no e-mail addresses: Parley is public"
        if STREET.search(text):
            return "no street addresses: Parley is public"
        if PHONE.search(text):
            return "no phone numbers: Parley is public"
        for m in URL.finditer(text):
            scheme = m.group(1).lower()
            if scheme not in ("http", "https") and m.group(2):
                return f"only http(s) links ({scheme}: is not allowed)"
        return None

    def check_text(self, what: str, text: str, *, author: str | None = None) -> None:
        """Raise Rejected if this text may not be published."""
        text = (text or "").strip()
        if not text:
            return
        reason = self.local(text)
        if reason is None and self.llm is not None:
            reason = self._ask(what, [{"type": "text", "text": f"The {what} to check:\n<<<\n{text[:4000]}\n>>>"}], author)
        if reason:
            self._log(what, author, reason, text)
            raise Rejected(reason)

    # -- pictures ---------------------------------------------------------
    def check_picture(self, what: str, data: bytes, mime: str, text: str = "", *, author: str | None = None) -> None:
        """Raise Rejected unless this picture, with the words shown with it, may be published."""
        reason = self.local(text)
        if reason is None:
            if self.llm is None:
                if self.require_images:
                    reason = "pictures cannot be checked right now, so none are posted"
            else:
                media = mime if mime in ("image/jpeg", "image/png", "image/gif", "image/webp") else None
                if media is None:
                    reason = "only JPEG, PNG, GIF and WebP pictures are checked"
                else:
                    content = [
                        {"type": "image", "source": {"type": "base64", "media_type": media, "data": base64.b64encode(data).decode()}},
                        {"type": "text", "text": f"The {what} to check: the picture above" + (f", shown with these words:\n<<<\n{text[:2000]}\n>>>" if text else ", shown with no words.")},
                    ]
                    reason = self._ask(what, content, author, image=True)
        if reason:
            self._log(what, author, reason, text or f"(picture {hashlib.sha256(data).hexdigest()[:12]})")
            raise Rejected(reason)

    # -- the model --------------------------------------------------------
    def _ask(self, what: str, content: list, author: str | None, image: bool = False) -> str | None:
        system = (RULES + "\n\nYou are Parley's moderator. Decide whether the " + what + " may be published. "
                  "Answer with ok (true or false) and a reason of at most 15 words that the author can act on.")
        try:
            resp, usage = self.llm.create(
                max_tokens=1024, system=system, messages=[{"role": "user", "content": content}],
                output_config={"effort": "low", "format": {"type": "json_schema", "schema": VERDICT}})
        except BudgetExceeded:
            return "today's moderation budget is spent; try again tomorrow" if (image or self.require_images) else None
        except Exception as e:  # the API failed: refuse pictures, let text pass on the local rules
            return f"the picture check failed ({type(e).__name__}); try again later" if image else None
        try:
            self.llm.record(kind="moderation", usage=usage, agent=author, summary=what)
        except Exception:
            pass
        text = "".join(getattr(b, "text", "") for b in resp.content if getattr(b, "type", "") == "text")
        if getattr(resp, "stop_reason", "") == "refusal":
            return "refused by the safety check"
        try:
            v = json.loads(text)
        except Exception:
            return "the check gave no clear answer, so it is refused" if image else None
        return None if v.get("ok") else (str(v.get("reason") or "against Parley's rules")[:200])

    def _log(self, what: str, author: str | None, reason: str, excerpt: str) -> None:
        from .service import new_id
        try:
            self.s.write([{"op": "insert", "table": "mod_log", "doc": {
                "id": new_id("m"), "what": what, "author": author, "verdict": "reject", "reason": reason[:300], "excerpt": (excerpt or "")[:300]}}],
                label="mod_log")
        except Exception:
            pass


def no_usage(model: str = "") -> Usage:
    return Usage(model)
