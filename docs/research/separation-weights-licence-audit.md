---
description: Licence and provenance audit of current music source-separation checkpoints for GPL-3.0-or-later Suhina.
read_when: Deciding whether Suhina may bundle or auto-download a separation checkpoint.
type: plan
status: draft
last_verified: 2026-10-13
covers: []
---

# Separation checkpoint licence audit (checked 2026-10-13)

Scope: current, widely used music/source-separation checkpoints. Verified = fetched the
rights holder's own page/file (licence text, model card, issue thread, Zenodo record).
Inferred = reasoning from a verified fact. Third-party/mirror licence tags are never
treated as a grant.

## Table

| Checkpoint | URL | Host | Weight licence as stated by rights holder | Training data as stated | MUSDB18-derived | Verdict |
|---|---|---|---|---|---|---|
| MUSDB18 corpus (baseline licence) | [zenodo 1117372](https://zenodo.org/records/1117372) | Zenodo | "Other (Non-Commercial)"; "provided for educational purposes only … should not be used for any commercial purpose without the express permission of the copyright holders" | n/a | itself | Not usable |
| Demucs v4 code + `htdemucs`, `htdemucs_ft`, `htdemucs_6s`, `hdemucs_mmi`, `mdx`, `mdx_extra`, `mdx_q`, `mdx_extra_q` | [facebookresearch/demucs](https://github.com/facebookresearch/demucs), [issue 327](https://github.com/facebookresearch/demucs/issues/327) | Meta / GitHub | Code MIT ([LICENSE](https://raw.githubusercontent.com/facebookresearch/demucs/main/LICENSE)); weights "are not covered by the MIT license, and are provided only for scientific purposes" | "trained on the MUSDB HQ dataset + an extra training dataset of 800 songs" ([README](https://raw.githubusercontent.com/facebookresearch/demucs/main/README.md)); the 800 songs are unnamed | Yes | Not usable |
| ZFTurbo MSST releases (MDX23C, SCNet, BS-Roformer, BandIt Plus, DTTNet, …) | [repo](https://github.com/ZFTurbo/Music-Source-Separation-Training), [model list](https://raw.githubusercontent.com/ZFTurbo/Music-Source-Separation-Training/main/docs/pretrained_models.md) | GitHub Releases | Code MIT (Roman Solovyev). Weights: **no licence stated anywhere**; the release/modellist page names no term | Named per row (mostly "MUSDB18HQ only", some with MUSDB test set) | Yes (most rows) | Not usable |
| Bandit v2 `checkpoint-{eng,deu,fra,spa,cmn,fao,multi}.ckpt` | [zenodo 12701995](https://zenodo.org/records/12701995), [repo](https://github.com/kwatcharasupat/bandit-v2) | Zenodo / Netflix + GaTech authors; code Apache-2.0 | **CC-BY-SA-4.0** (Zenodo record rights field, dataset-level grant by the depositing authors) | "Remastering Divide and Remaster" → DnR | No | Usable with conditions |
| DnR dataset (Bandit v2 training data) | [zenodo 5574713](https://zenodo.org/records/5574713) | Zenodo | **CC-BY-4.0** | LIBRISPEECH + FMA + FSD50K (see caveat) | No | Usable with conditions |
| Bandit Plus (`model_bandit_plus_dnr_sdr_11.47.chpt`) | [MSST release v1.0.3](https://github.com/ZFTurbo/Music-Source-Separation-Training/releases/tag/v.1.0.3) | GitHub Releases | No licence stated | DnR | No | Not usable |
| SCNet / SCNet-XL (starrytong) | [repo](https://github.com/starrytong/SCNet) | GitHub + Google Drive | Code MIT (c) 2024 starrytong, verified in [LICENSE](https://raw.githubusercontent.com/starrytong/SCNet/main/LICENSE). Weights: "The model checkpoint was trained on the MUSDB dataset. You can download it from the following link" — no licence, Drive link only | MUSDB | Yes | Not usable |
| SCNet large/XL redistributions | [MSST releases](https://github.com/ZFTurbo/Music-Source-Separation-Training/releases) | GitHub Releases | No licence stated | MUSDB18HQ | Yes | Not usable |
| KimberleyJSN `melbandroformer` (`MelBandRoformer.ckpt`, "Kim Vocal 2") | [HF repo](https://huggingface.co/KimberleyJSN/melbandroformer) | HF, author KimberleyJSN | Card says `license: mit` (bare two-line card, **no licence text**), but the owner wrote in [discussion #1](https://huggingface.co/KimberleyJSN/melbandroformer/discussions/1): "I released the model weights so people can use it for personal / research usage" | **Silent** — card declares no dataset | Unknown | Not usable |
| pcunwa family (BS-Roformer-1297/1298, Revive, Resurrection, Leap, HyperACE, Large-Inst, Mel-Band-Roformer-big/Inst/…) | [pcunwa on HF](https://huggingface.co/pcunwa/BS-Roformer-Revive) | HF, author pcunwa | **No licence tag on any of the 13 repos**; Revive card is two sentences ("This is a fine-tuning of the Viperx 1297 model") | Silent | Unknown (derived from Viperx 1297) | Not usable |
| `viperx` 1297/1298 + Mel-Band-Roformer ep3005 | [TRvlvr/model_repo](https://github.com/TRvlvr/model_repo) (linked from [MSST model list](https://raw.githubusercontent.com/ZFTurbo/Music-Source-Separation-Training/main/docs/pretrained_models.md)) | GitHub Releases | **No LICENSE file, no term stated** (verified: repo README 404, HF mirror `TRvlvr/model_repo` 401) | Silent | Unknown | Not usable |
| jarredou `BS-ROFO-SW-Fixed` | original repo gone; third-party copies only: [enerjazzer](https://huggingface.co/enerjazzer/BS-ROFO-SW-Fixed) (`license:unknown`), [Blakus](https://huggingface.co/Blakus/bs_roformer_sw_6stem) (`license:mit`) | HF (third parties) | **No rights-holder grant exists.** Rights holder's repo is 401/gone; mirrors make conflicting claims | Silent | Unknown | Not usable |
| anvuew `dereverb_mel_band_roformer`, `dereverb_bs_roformer`, `karaoke_bs_roformer`, `BS-RoFormer`, `BS_RoFormer_mag` | [HF](https://huggingface.co/anvuew/dereverb_mel_band_roformer) | HF, author anvuew | `license: gpl-3.0` (model card YAML, authored by the rights holder) | Dereverb card: "训练数据的干声为单声道单人演唱/说话" (mono solo voice) — **dataset never named**; BS-RoFormer card says only "dataset by bascurtiz" | Unknown | Not usable |
| becruily `mel-band-roformer-{vocals,instrumental,karaoke,guitar,deux,invert-clean}`, `bs-roformer-{karaoke,delimit}` | [HF](https://huggingface.co/becruily/mel-band-roformer-vocals) | HF, author becruily | Inconsistent: vocals/instrumental/karaoke/guitar/invert-clean have **no licence tag**; `deux` and `delimit` are `cc-by-nc-4.0` | Silent | Unknown | Not usable |
| Sucial / SUC-DriverOld (Aspiration, Dereverb-Echo, Chorus-Male-Female, MSST-WebUI bundle) | [HF](https://huggingface.co/Sucial/Aspiration_Mel_Band_Roformer), [MSST-WebUI](https://huggingface.co/Sucial/MSST-WebUI) | HF, author Sucial | `cc-by-nc-sa-4.0` — non-commercial, GPL-incompatible | Silent | Unknown | Not usable |
| HiDolen `Mini-BS-RoFormer` (8.8M) | [HF](https://huggingface.co/HiDolen/Mini-BS-RoFormer) | HF, author HiDolen | `license: mit`, declared by the author's card | "使用 MUSDB18HQ 数据进行训练" (MUSDB18-HQ only) | Yes | Not usable |
| HiDolen `Mini-BS-RoFormer-18M` | [HF](https://huggingface.co/HiDolen/Mini-BS-RoFormer-18M) | HF | `license: mit` | MUSDB18-HQ | Yes | Not usable |
| HiDolen `Mini-BS-RoFormer-V2-46.8M` | [HF](https://huggingface.co/HiDolen/Mini-BS-RoFormer-V2-46.8M) | HF | `license: cc-by-nc-4.0` | "MUSDB18HQ 数据集的 train 和 test 集" | Yes | Not usable |
| Intel OpenVINO conversions (`vocals_mel_band_roformer_kimberleyJSN`, `dereverb_…_anvuew`, `drumsep_mdx23c_jarredou`, `crowd_…_aufr33_viperx`) | [HF](https://huggingface.co/Intel/vocals_mel_band_roformer_kimberleyJSN_openvino) | HF, org Intel | Intel retags these `gpl-3.0` / `mit` / `cc-by-nc-sa-4.0` inconsistently — a **third-party** claim, not a grant | Inherited | Yes | Not usable |
| HTDemucs re-uploads (`adefossez/HTDemucs*`, StemSplitio, AEmotionStudio, gguf/onnx/coreml ports) | [adefossez/HTDemucs-ft](https://huggingface.co/adefossez/HTDemucs-ft) | HF, many third parties | Most tag `license: mit`; the original author's own HF repos carry **no licence tag**. A mirror's MIT tag does not grant the weights | Inherited (MUSDB + 800 songs) | Yes | Not usable |

## Provenance defects found (derivative / silent)

- **Derivative chain.** `htdemucs_ft` is a fine-tune of `htdemucs`; HiDolen Mini-BS-RoFormer
  models are fine-tunes of `lucidrains/BS-RoFormer` code; pcunwa Revive is explicitly "a
  fine-tuning of the Viperx 1297 model"; anvuew's dereverb is "微调自 kim的人声乐器分离模型"
  (a fine-tune of KimberleyJSN/melbandroformer) — so it inherits a kit whose own author limits
  it to "personal / research usage". Every derivative inherits the upstream term.
- **Silence = blocker.** pcunwa (13 repos), viperx/TRvlvr, becruily (7 repos), jarredou, and
  all ZFTurbo-hosted third-party releases state **no training dataset at all**. Not a blank to
  fill in later: it means chain of title cannot be established.
- **Conflicting third-party relicensing is systemic.** Sucial/SUC-DriverOld redistributes the
  whole MSST+UVR model set as `cc-by-nc-sa-4.0`; Intel retags under GPL/MIT/NC; HF mirrors tag
  jarredou's model `mit` and `unknown` simultaneously. Same bytes, four claimed licences.

## Answer: is there a current high-quality checkpoint Suhina could legally bundle or auto-download?

**No — not with the evidence available on 2026-10-13.** Every current, high-quality checkpoint
fails at least one of the two required links:

- 18 of the widely used families fail at the **weights** link (no grant, or a non-commercial /
  GPL-incompatible grant, or a grant contradicted by its own author).
- The remainder fail at the **data** link (MUSDB18 is non-commercial by its own text, and the
  popular models are all MUSDB-derived) or are simply silent about data.
- Where a rights holder did grant a permissive weight licence *and* used permissive data
  (HiDolen Mini-BS-RoFormer: MIT weights, MUSDB18-HQ only), the data licence itself is the
  blocker — and the same author moved their later version to `cc-by-nc-4.0`, which reads as
  recognition of exactly that.

**Closest option, with its specific missing piece: Bandit v2** (`checkpoint-multi.ckpt` and
per-language variants). Weight grant: CC-BY-SA-4.0, stated by the rights holder on the Zenodo
record (Netflix/GaTech authors, 2024-07-09), linked from their own Apache-2.0 repo — the only
such explicit grant found. Data grant: DnR is CC-BY-4.0. **Missing piece / conditions:** (1) DnR
is built partly from FSD50K, whose own record says the clips are "some of them requiring
attribution … and some forbidding further commercial reuse" and that "the choice of one license
for the dataset as a whole is not straightforward … may warrant further investigation (e.g., by
someone with a background in copyright law)" — while DnR's license field is CC-BY-4.0, so an
NC-clip tail cannot be excluded; obtain written confirmation from Darius Petermann. (2) Bandit v2
separates **speech / music / effects** for cinematic remastering, and is multilingual — it is not
a 4-stem music separator, so it does not meet Suhina's actual feature need. Treat it as a
footnote, not a substitute.

Second-closest: **nothing**. There is no MUSDB-free, permissively-licensed music separator of
current quality that was verified here. Models trained on Mixing Secrets, MedleyDB, Slakh, or
MUSDB18-HQ-only either do not exist as current checkpoints with a stated weight licence, or exist
precisely as the MUSDB-derived rows above. (MUSDB18's own record names its provenance: 100 tracks
from the Mixing Secrets library, 46 from MedleyDB (CC-BY-NC-SA-4.0), so "Mixing Secrets only" is
not automatically permissive either.)

Recommended Suhina behaviour now: **do not bundle, and do not auto-download.** Ship no weights;
link out to the user's own installation, and record this audit. A SEPARATE, defensible option is
a runtime user-supplied model path with no licence assertion.

## Permission-request list

Verified contact points (all checked 2026-10-13):

| Who | Where to ask |
|---|---|
| Meta / Demucs weights | [demucs issue tracker](https://github.com/facebookresearch/demucs/issues) (issue 327 is the precedent thread) |
| ZFTurbo / MSST-hosted releases | [MSST issues](https://github.com/ZFTurbo/Music-Source-Separation-Training/issues), model-sharing thread [issue 1](https://github.com/ZFTurbo/Music-Source-Separation-Training/issues/1) |
| starrytong / SCNet | [SCNet issues](https://github.com/starrytong/SCNet/issues) |
| KimberleyJSN | [model discussion thread](https://huggingface.co/KimberleyJSN/melbandroformer/discussions/1) (owner replies) |
| pcunwa | [pcunwa HF discussions](https://huggingface.co/pcunwa/BS-Roformer-Revive/discussions) |
| TRvlvr / viperx UVR weights | [TRvlvr/model_repo issues](https://github.com/TRvlvr/model_repo/issues) |
| jarredou | [jarredou/models issues](https://github.com/jarredou/models/issues) |
| becruily, Sucial, anvuew, HiDolen | HF discussions tab on each repo (e.g. [anvuew](https://huggingface.co/anvuew/dereverb_mel_band_roformer/discussions)) |
| Bandit v2 / DnR data | [bandit-v2 issues](https://github.com/kwatcharasupat/bandit-v2/issues), [dnr-utils issues](https://github.com/darius522/dnr-utils/issues) |
| UVR application | [ultimatevocalremovergui issues](https://github.com/Anjok07/ultimatevocalremovergui/issues) |

What a request must ask for, in writing and explicitly:

1. A licence **for the weights** as a separate subject matter from the code — state plainly that
   the code's MIT/Apache licence is understood not to cover them.
2. **GPL-3.0-or-later compatibility**: permission to distribute the weights, and derivatives, as
   part of a GPL-3.0-or-later work, including in object/binary form.
3. **Commercial use** and use in a paid or free product, with no field-of-use restriction
   (not "research only", not "scientific purposes", not NC).
4. **Redistribution and auto-download** rights, including mirroring by third parties and hosting
   by Suhina, plus permission to publish checksums.
5. The **exact training data** used, with each source dataset, its version, its licence, and the
   split used; confirmation that no NC or "no commercial use" material is included.
6. **Chain of title** for any upstream model: if it is a fine-tune, the upstream rights holder's
   written grant on the same terms.
7. **Attribution and notice requirements** Suhina must reproduce, in a form usable from a
   third-party notices file.
8. Confirmation that the grantor holds the rights they are granting, and an indemnity or at least
   a statement of who bears the risk.
