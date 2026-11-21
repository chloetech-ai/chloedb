"""A more lenient answer check than containment, for answers that are right but shorter or worded differently
("18" for "18 kinds of object", "by their _id" for "the order of their _id"): numbers lose their commas, and an
answer is right when, after stop words go, its words are a subset of the gold's or the gold's of its (at least
one content word in common). Spot-checked by hand; not a judge model."""
import re
STOP = set("a an the of in on to by for and or is are be it its their his her this that with as at from per into default defaults ChloeDB chloedb".lower().split())
def words(t):
    t = re.sub(r"(?<=\d),(?=\d)", "", t.lower()).replace("`", "")
    return [w for w in re.findall(r"[a-z0-9_./:-]+", t) if w not in STOP]
def lenient(ans, gold):
    a, g = set(words(ans)), set(words(gold))
    return bool(a and g and (a <= g or g <= a))
