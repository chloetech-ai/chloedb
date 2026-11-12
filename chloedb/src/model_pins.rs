//! The Hugging Face revision of each model ChloeDB knows by name
//! ([`crate::embed::KNOWN_MODELS`], [`crate::derive::KNOWN_MODELS`],
//! [`crate::query::cross_encoder::KNOWN_MODELS`]), and the SHA-256 of each
//! file fetched from it: `chloedb-server fetch-model` downloads that
//! commit, not `main`, and refuses a file whose bytes differ, so that a
//! model changed upstream under the same name cannot slip into a database
//! whose vectors came from the old one. Looked up on 2026-11-10 from
//! `https://huggingface.co/api/models/<repo>?blobs=true` (LFS files) and by
//! downloading the others at the revision.

/// A repository's pinned commit and its files' SHA-256, hex.
pub struct Pin {
    pub repo: &'static str,
    pub revision: &'static str,
    pub files: &'static [(&'static str, &'static str)],
}

impl Pin {
    /// The SHA-256 of file `path` of the repository, if pinned.
    pub fn sha256(&self, path: &str) -> Option<&'static str> {
        self.files.iter().find(|f| f.0 == path).map(|f| f.1)
    }
}

/// The pin of repository `repo` (`owner/name`, any case).
pub fn pin(repo: &str) -> Option<&'static Pin> {
    PINS.iter().find(|p| p.repo.eq_ignore_ascii_case(repo))
}

/// SHA-256 of `bytes`, hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub const PINS: &[Pin] = &[
    Pin {
        repo: "minishlab/potion-retrieval-32M",
        revision: "6fc8051fab2a1e0ee76689cf08c853792ac285e7",
        files: &[
            ("config.json", "63c00d90824c832c04ec1d02b6a983fb90489bf049f29fbff15ba481b8a432ee"),
            ("tokenizer.json", "7d75cbc54318138807c401b0f0c9721117c628b39de8e8e0edb6cb17e0ee7d18"),
            ("model.safetensors", "07609e5bd33aad37900b3fd62f4ec96f6daec88ca4d46b9d8b928bfababf6ea0"),
        ],
    },
    Pin {
        repo: "minishlab/potion-base-8M",
        revision: "bf8b056651a2c21b8d2565580b8569da283cab23",
        files: &[
            ("config.json", "2a6ac0e9aaa356a68a5688070db78fc3a464fefe85d2f06a1905ce3718687553"),
            ("tokenizer.json", "e67e803f624fb4d67dea1c730d06e1067e1b14d830e2c2202569e3ef0f70bb50"),
            ("model.safetensors", "f65d0f325faadc1e121c319e2faa41170d3fa07d8c89abd48ca5358d9a223de2"),
        ],
    },
    Pin {
        repo: "BAAI/bge-small-en-v1.5",
        revision: "5c38ec7c405ec4b44b94cc5a9bb96e735b38267a",
        files: &[
            ("config.json", "094f8e891b932f2000c92cfc663bac4c62069f5d8af5b5278c4306aef3084750"),
            ("tokenizer.json", "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66"),
            ("model.safetensors", "3c9f31665447c8911517620762200d2245a2518d6e7208acc78cd9db317e21ad"),
        ],
    },
    Pin {
        repo: "Snowflake/snowflake-arctic-embed-m-v1.5",
        revision: "e58a8f756156a1293d763f17e3aae643474e9b8a",
        files: &[
            ("config.json", "d2dddc06af0aeeb7fd3289a3dee1ac76dce0c3df2467cadd26f5f731f8d7919b"),
            ("tokenizer.json", "91f1def9b9391fdabe028cd3f3fcc4efd34e5d1f08c3bf2de513ebb5911a1854"),
            ("model.safetensors", "463d27a3e88a748997316367c419bbdab4d809300a5f24b9b19ba9b2dc08ffdd"),
        ],
    },
    Pin {
        repo: "google/siglip-base-patch16-224",
        revision: "7fd15f0689c79d79e38b1c2e2e2370a7bf2761ed",
        files: &[
            ("config.json", "cd85b3d28829722820bcb89a2cfbb4160e55fd359249a3044da724166a8d9688"),
            ("tokenizer.json", "c6e405cb7c670d56636a9402c81023a55bc6c3c53d89cf02b92f5c5005bfe920"),
            ("model.safetensors", "2c63cb7d1f2e95ba501893cbb8faeb4ea9a3af295498d35097126228659c2af8"),
        ],
    },
    Pin {
        repo: "laion/clap-htsat-unfused",
        revision: "8fa0f1c6d0433df6e97c127f64b2a1d6c0dcda8a",
        files: &[
            ("config.json", "9efb9557bc804f2ca6e394486af2e45dfed0b18554909735a99c6220b84e4288"),
            ("tokenizer.json", "77ef92283d67f0d97e1454909a964afcbfa2019f0fb9f18f8e88d5c25c3ba729"),
            ("pytorch_model.bin", "1cd3c601bc4afe0fa87be3de4c13dd2cfadd249fac1e29acf74a9b296c3219bb"),
        ],
    },
    Pin {
        repo: "answerdotai/answerai-colbert-small-v1",
        revision: "934fa8bb4ce2284f4c2baa232d81aca4d076fa5e",
        files: &[
            ("config.json", "8a7ef3067ec44d7909d28daaf0cd52a6739a25e689252789b0927c593e17b121"),
            ("tokenizer.json", "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66"),
            ("model.safetensors", "35bb6ef696cc88b3d18b7520273dfd25315f6c47d265142ba13bba38678c92b6"),
            ("artifact.metadata", "f09c7e00cde4fda0b272b47e07d537d7b698756d0d5ffcf8b82ec17553d1b4e4"),
        ],
    },
    Pin {
        repo: "Salesforce/blip-image-captioning-base",
        revision: "82a37760796d32b1411fe092ab5d4e227313294b",
        files: &[
            ("config.json", "f7846f82f4e2c4a2ccbab9ae8b0e44873540a271a76a1288effd078180c13a82"),
            ("tokenizer.json", "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66"),
            ("pytorch_model.bin", "d6638651a5526cc2ede56f2b5104d6851b0755816d220e5e046870430180c767"),
        ],
    },
    Pin {
        repo: "openai/whisper-small",
        revision: "973afd24965f72e36ca33b3055d56a652f456b4d",
        files: &[
            ("config.json", "e6a2b489da1b5aed65a8eb8d1e7466fa867ad5643a8bc138ba708bd56b2875c4"),
            ("tokenizer.json", "27fc476bfe7f17299480be2273fc0608e4d5a99aba2ab5dec5374b4482d1a566"),
            ("generation_config.json", "71565b8ef50d0bf7a1193ed4bbed195b94e70c18894d81bba2f1233dcec3ab53"),
            ("model.safetensors", "1d7734884874f1a1513ed9aa760a4f8e97aaa02fd6d93a3a85d27b2ae9ca596b"),
        ],
    },
    Pin {
        repo: "cross-encoder/ms-marco-MiniLM-L6-v2",
        revision: "233902d25c440f23af6f7d6e94d2946bac0bee0a",
        files: &[
            ("config.json", "380e02c93f431831be65d99a4e7e5f67c133985bf2e77d9d4eba46847190bacc"),
            ("tokenizer.json", "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66"),
            ("model.safetensors", "821d1aa69520101d6e0737f78a042ae25b19e5cb9160701909d10434f4aeb0ae"),
            ("onnx/model_qint8_arm64.onnx", "3573b6b9593cb2f75987a31815d409ca3dd8808629118fd20451bb1a5d90cec7"),
            ("onnx/model_qint8_avx512_vnni.onnx", "3573b6b9593cb2f75987a31815d409ca3dd8808629118fd20451bb1a5d90cec7"),
            ("onnx/model_quint8_avx2.onnx", "c80a8b34256ea453093d612e3ac48d3d965a0c0a48c7906709af8b8e28461bf9"),
        ],
    },
    Pin {
        repo: "BAAI/bge-reranker-base",
        revision: "2cfc18c9415c912f9d8155881c133215df768a70",
        files: &[
            ("config.json", "289adf7ada1eb6b4afa7589a48a032d45a076cf2e46dcdb3b4cabc33be14f708"),
            ("tokenizer.json", "9eb652ac4e40cc093272bbbe0f55d521cf67570060227109b5cdc20945a4489e"),
            ("model.safetensors", "ced967c45fd1902eb92716c9ceeca7c95a936770ea9db611f5a841b926e33fbd"),
        ],
    },
    Pin {
        repo: "Alibaba-NLP/gte-reranker-modernbert-base",
        revision: "f7481e6055501a30fb19d090657df9ec1f79ab2c",
        files: &[
            ("config.json", "c9316ff715158502dad782f35454eee18de984160618dc30afe7508feb46b7ce"),
            ("tokenizer.json", "2aea6ff4701d063e7e029b6be695a1659f2caaa2ae4fb0e8b18285818271becd"),
            ("model.safetensors", "13c533d902d6a48fa97375c6856c3e60854f9228ea760e3fc18ebde057e0cf31"),
            ("onnx/model_int8.onnx", "ecc6a0ae67cee3d898167802383112d9185ca9250e07bd5d1fa65019b050179d"),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every known model's repository is pinned, with every file a fetch
    /// takes from it.
    #[test]
    fn every_known_model_is_pinned() {
        let known = crate::embed::KNOWN_MODELS.iter().chain(crate::derive::KNOWN_MODELS).chain(crate::query::cross_encoder::KNOWN_MODELS);
        for m in known {
            let p = pin(m.1).unwrap_or_else(|| panic!("{} is not pinned", m.1));
            assert_eq!(p.revision.len(), 40);
            let files: Vec<&str> = match (crate::derive::KNOWN_MODELS.iter().any(|d| d.0 == m.0), crate::query::cross_encoder::fetch_files(m.0)) {
                (true, _) => crate::derive::model_files(m.0).to_vec(),
                (false, Some(f)) => f.iter().map(|x| x.0).collect(),
                (false, None) => crate::embed::model_files(m.0).to_vec(),
            };
            for f in files {
                let h = p.sha256(f).unwrap_or_else(|| panic!("{}: {f} is not pinned", m.1));
                assert!(h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()), "{}: {f}", m.1);
            }
        }
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
