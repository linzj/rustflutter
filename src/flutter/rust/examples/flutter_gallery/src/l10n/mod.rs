// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Mirrors upstream `lib/l10n/` (flutter/gallery @ d12640d).
//!
//! English-only per PORTING.md: `gallery_localizations` is the lookup surface
//! and `gallery_localizations_en` the one real table; the other 76 modules
//! are generated placeholders marking where each upstream locale file stands.

// The catalogue surface is ported ahead of its call sites; the screens that
// read it are later batches.
#![allow(dead_code)]

pub mod gallery_localizations;
pub mod gallery_localizations_af;
pub mod gallery_localizations_am;
pub mod gallery_localizations_ar;
pub mod gallery_localizations_as;
pub mod gallery_localizations_az;
pub mod gallery_localizations_be;
pub mod gallery_localizations_bg;
pub mod gallery_localizations_bn;
pub mod gallery_localizations_bs;
pub mod gallery_localizations_ca;
pub mod gallery_localizations_cs;
pub mod gallery_localizations_cy;
pub mod gallery_localizations_da;
pub mod gallery_localizations_de;
pub mod gallery_localizations_el;
pub mod gallery_localizations_en;
pub mod gallery_localizations_es;
pub mod gallery_localizations_et;
pub mod gallery_localizations_eu;
pub mod gallery_localizations_fa;
pub mod gallery_localizations_fi;
pub mod gallery_localizations_fil;
pub mod gallery_localizations_fr;
pub mod gallery_localizations_gl;
pub mod gallery_localizations_gsw;
pub mod gallery_localizations_gu;
pub mod gallery_localizations_he;
pub mod gallery_localizations_hi;
pub mod gallery_localizations_hr;
pub mod gallery_localizations_hu;
pub mod gallery_localizations_hy;
pub mod gallery_localizations_id;
pub mod gallery_localizations_is;
pub mod gallery_localizations_it;
pub mod gallery_localizations_ja;
pub mod gallery_localizations_ka;
pub mod gallery_localizations_kk;
pub mod gallery_localizations_km;
pub mod gallery_localizations_kn;
pub mod gallery_localizations_ko;
pub mod gallery_localizations_ky;
pub mod gallery_localizations_lo;
pub mod gallery_localizations_lt;
pub mod gallery_localizations_lv;
pub mod gallery_localizations_mk;
pub mod gallery_localizations_ml;
pub mod gallery_localizations_mn;
pub mod gallery_localizations_mr;
pub mod gallery_localizations_ms;
pub mod gallery_localizations_my;
pub mod gallery_localizations_nb;
pub mod gallery_localizations_ne;
pub mod gallery_localizations_nl;
pub mod gallery_localizations_or;
pub mod gallery_localizations_pa;
pub mod gallery_localizations_pl;
pub mod gallery_localizations_pt;
pub mod gallery_localizations_ro;
pub mod gallery_localizations_ru;
pub mod gallery_localizations_si;
pub mod gallery_localizations_sk;
pub mod gallery_localizations_sl;
pub mod gallery_localizations_sq;
pub mod gallery_localizations_sr;
pub mod gallery_localizations_sv;
pub mod gallery_localizations_sw;
pub mod gallery_localizations_ta;
pub mod gallery_localizations_te;
pub mod gallery_localizations_th;
pub mod gallery_localizations_tl;
pub mod gallery_localizations_tr;
pub mod gallery_localizations_uk;
pub mod gallery_localizations_ur;
pub mod gallery_localizations_uz;
pub mod gallery_localizations_vi;
pub mod gallery_localizations_zh;
pub mod gallery_localizations_zu;
