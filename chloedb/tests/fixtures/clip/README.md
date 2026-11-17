# CLIP fixtures

`portrait.jpg` is a 353 by 560 thumbnail of a public-domain drawing from
Wikimedia Commons, by way of PD12M
(`https://pd12m.s3.us-west-2.amazonaws.com/images/c292d21b-835c-5c63-ba8e-59012ab99c0b.jpeg`,
Public Domain Mark 1.0). `reference.json` holds the vectors that OpenAI's
CLIP ViT-L/14 gives its texts and that picture through Hugging Face's
transformers, which `tests/clip.rs` compares ChloeDB's own with.
