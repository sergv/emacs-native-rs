// Copyright 2021 Sergey Vinokurov
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(dead_code)]

type Heat = i16;

const LAST_CHAR_BONUS: Heat = 1;
const INIT_SCORE: Heat = -35;
const LEADING_PENALTY: Heat = -45;
const WORD_START: Heat = 85;

pub fn heat_map<'a>(
    s: &str,
    group_seps: &[char], // sorted
    mut heatmap: &'a mut Vec<Heat>,
) -> &'a mut Vec<Heat> {
    heatmap.clear();
    if s.is_empty() {
        return heatmap;
    }

    let mut group_idx = 0;

    let mut is_base_path = false;
    let mut group_start: isize = 0;
    let mut group_end: isize = -1; // to account for fake separator
    let mut group_score = 0;
    let mut group_non_base_score = 0;

    let split = split_with_seps(' ', s, group_seps);

    let groups_count = match &split {
        Ok((_, _)) => 1,
        Err(groups) => groups.len() as i16,
    };

    let init_score_adjustment = if groups_count > 1 { -2 * groups_count } else { 0 };

    heatmap.resize(s.chars().count(), INIT_SCORE + init_score_adjustment);
    *heatmap.last_mut().unwrap() += LAST_CHAR_BONUS;

    match split {
        Ok((prev, text)) => {
            analyze_group(
                prev,
                text,
                &mut heatmap,
                &mut is_base_path,
                &mut group_idx,
                groups_count,
                &mut group_start,
                &mut group_end,
                &mut group_score,
                &mut group_non_base_score,
            )
        },
        Err(groups) => {
            let mut prev_is_base_path = false;
            let mut prev_group_start = 0;
            let mut prev_group_end = 0;
            let mut prev_group_score = 0;
            let mut prev_group_non_base_score = 0;
            for (prev, text) in groups {
                analyze_group(
                    prev,
                    text,
                    &mut heatmap,
                    &mut is_base_path,
                    &mut group_idx,
                    groups_count,
                    &mut group_start,
                    &mut group_end,
                    &mut group_score,
                    &mut group_non_base_score,
                );

                if prev_is_base_path && is_base_path {
                    let delta = prev_group_non_base_score - prev_group_score;
                    apply_group_score(delta, heatmap, prev_group_start, prev_group_end);
                    prev_is_base_path = false;
                }

                if is_base_path {
                    prev_is_base_path = is_base_path;
                    prev_group_start = group_start;
                    prev_group_end = group_end;
                    prev_group_score = group_score;
                    prev_group_non_base_score = group_non_base_score;
                }
            }
        }
    };

    heatmap
}

fn apply_group_score(score: Heat, heatmap: &mut [Heat], start: isize, end: isize) {
    for i in start..end {
        heatmap[i as usize] += score;
    }
    if end < heatmap.len() as isize {
        heatmap[end as usize] += score;
    }
}

fn analyze_group(
    mut prev: char,
    text: &str,
    heatmap: &mut [Heat],
    is_base_path: &mut bool,
    group_idx: &mut i16,
    groups_count: i16,
    group_start: &mut isize,
    group_end: &mut isize,
    group_score: &mut Heat,
    group_non_base_score: &mut Heat,
) {
    let mut word_char_idx = 0;
    let mut word_idx = -1;
    let mut word_count = 0;

    *group_start = *group_end + 1;

    let mut chars_count = 0;

    for (i, c) in text.chars().enumerate() {
        let j = *group_start as usize + i;
        let is_word = !is_word(prev) && is_word(c);
        if is_word {
            word_count += 1;
        }

        let is_boundary = is_word || !prev.is_uppercase() && c.is_uppercase();
        if is_boundary {
            word_idx += 1;
            word_char_idx = 0;
            heatmap[j] += WORD_START;
        }

        if word_idx >= 0 {
            heatmap[j] += (-3) * word_idx - word_char_idx;
        }

        word_char_idx += 1;
        if penalizes_if_leading(c) {
            let k = j + 1;
            if k < heatmap.len() {
                heatmap[k] += LEADING_PENALTY;
            }
        }
        prev = c;
        chars_count += 1;
    }

    *group_end = *group_start + chars_count;

    // Update score for trailing separator of a group.
    let k = *group_end as usize;
    if k < heatmap.len() && word_idx >= 0 {
        heatmap[k] += (-3) * word_idx - word_char_idx;
    }

    let base_path = word_count != 0;
    *is_base_path = base_path;
    *group_score = calc_group_score(base_path, groups_count, word_count, *group_idx);
    if base_path {
        *group_non_base_score =
            calc_group_score(false, groups_count, word_count, *group_idx);
    }

    *group_idx += 1;

    apply_group_score(*group_score, heatmap, *group_start, *group_end);
}

struct SplitWithSeps<'a, 'b> {
    prev: char,
    s: &'a str,
    group_seps: &'b [char],
}

impl<'a, 'b> SplitWithSeps<'a, 'b> {
    fn is_sep(&self, c: char) -> bool {
        is_member(c, self.group_seps)
    }
}

impl<'a, 'b> Iterator for SplitWithSeps<'a, 'b> {
    type Item = (char, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        match self.s.split_once(|c| self.is_sep(c)) {
            None => {
                if self.s.is_empty() {
                    None
                } else {
                    let tmp = self.s;
                    self.s = "";
                    Some((self.prev, tmp))
                }
            },
            Some((prefix, suffix)) => {
                let sep = self.s[prefix.len()..].chars().next().unwrap();
                let tmp = self.prev;
                self.prev = sep;
                self.s = suffix;
                Some((tmp, prefix))
            }
        }
    }
}

impl<'a, 'b> ExactSizeIterator for SplitWithSeps<'a, 'b> {
    fn len(&self) -> usize {
        self.s.chars().filter(|c| self.is_sep(*c)).count() + 1
    }
}

fn split_with_seps<'a, 'b>(
    first_sep: char,
    s: &'a str,
    group_seps: &'b [char],
) -> Result<(char, &'a str), SplitWithSeps<'a, 'b>>
{
    if group_seps.is_empty() {
        Ok((first_sep, s))
    } else {
        // Assert that group_seps is sorted.
        debug_assert!(
            group_seps
                .iter()
                .fold((group_seps[0], true), |(prev, is_sorted), &c| (c, is_sorted && prev <= c))
                .1
        );
        Err(SplitWithSeps {
            prev: first_sep,
            s,
            group_seps
        })
    }
}

fn calc_group_score(is_base_path: bool, groups_count: i16, word_count: i16, n: i16) -> Heat {
    if is_base_path {
        35 + (groups_count - 2).max(0) - word_count
    } else {
        let delta = if n == 0 {
            -3
        } else {
            -6 + n
        };
        delta
    }
}

fn is_member(c: char, xs: &[char]) -> bool {
    xs.binary_search(&c).is_ok()
}

fn is_word_separator(c: char) -> bool {
    match c {
        ' ' | '*' | '+' | '-' | '_' | ':' | ';' | '.' | '/' | '\\' => true,
        _ => false,
    }
}

fn is_word(c: char) -> bool {
    !is_word_separator(c)
}

fn is_capital(c: char) -> bool {
    is_word(c) && c.is_uppercase()
}

fn penalizes_if_leading(c: char) -> bool {
    c == '.'
}

#[test]
fn test_heat_map1() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo", &[], &mut v), &mut vec![84, -2, -2]);
}

#[test]
fn test_heat_map2() {
    let mut v = Vec::new();
    assert_eq!(heat_map("bar", &[], &mut v), &mut vec![84, -2, -2]);
}

#[test]
fn test_heat_map3() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo.bar", &[], &mut v), &mut vec![83, -3, -4, -5, 35, -6, -6]);
}

#[test]
fn test_heat_map4() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo/bar/baz", &[], &mut v), &mut vec![82, -4, -5, -6, 79, -7, -8, -9, 76, -10, -10]);
}

#[test]
fn test_heat_map5() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo/bar/baz", &['/'], &mut v), &mut vec![41, -45, -46, -47, 39, -47, -48, -49, 79, -7, -7]);
}

#[test]
fn test_heat_map6() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo/bar+quux/fizz.buzz/frobnicate/frobulate", &[], &mut v), &mut vec![78, -8, -9, -10, 75, -11, -12, -13, 72, -14, -15, -16, -17, 69, -17, -18, -19, -20, 21, -20, -21, -22, -23, 63, -23, -24, -25, -26, -27, -28, -29, -30, -31, -32, 60, -26, -27, -28, -29, -30, -31, -32, -32]);
}

#[test]
fn test_heat_map7() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo/bar+quux/fizz.buzz/frobnicate/frobulate", &['/'], &mut v), &mut vec![37, -49, -50, -51, 35, -51, -52, -53, 32, -54, -55, -56, -57, 36, -50, -51, -52, -53, -12, -53, -54, -55, -56, 37, -49, -50, -51, -52, -53, -54, -55, -56, -57, -58, 77, -9, -10, -11, -12, -13, -14, -15, -15]);
}

#[test]
fn test_heat_map7a() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo/bar+quux/fizz.buzz", &['/'], &mut v),
               &mut vec![41, -45, -46, -47, 39, -47, -48, -49, 36, -50, -51, -52, -53, 78, -8, -9, -10, -11, 30, -11, -12, -12]);
}

#[test]
fn test_heat_map8() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo/bar+quux/fizz.buzz//frobnicate/frobulate", &['/'], &mut v), &mut vec![35, -51, -52, -53, 33, -53, -54, -55, 30, -56, -57, -58, -59, 34, -52, -53, -54, -55, -14, -55, -56, -57, -58, -50, 36, -50, -51, -52, -53, -54, -55, -56, -57, -58, -59, 76, -10, -11, -12, -13, -14, -15, -16, -16]);
}

#[test]
fn test_heat_map9() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo/bar+quux/fizz.buzz//frobnicate/frobulate", &['/', 'u'], &mut v), &mut vec![27, -59, -60, -61, 25, -61, -62, -63, 22, -64, -59, -58, -58, 28, -58, -59, -60, -61, -20, -61, -56, -56, -56, -55, 31, -55, -56, -57, -58, -59, -60, -61, -62, -63, -64, 72, -14, -15, -16, -17, -52, -52, -52, -51]);
}

#[test]
fn test_heat_map10() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo/barQuux/fizzBuzz//frobnicate/frobulate", &[], &mut v), &mut vec![80, -6, -7, -8, 77, -9, -10, 74, -12, -13, -14, -15, 71, -15, -16, -17, 68, -18, -19, -20, -21, -22, 65, -21, -22, -23, -24, -25, -26, -27, -28, -29, -30, 62, -24, -25, -26, -27, -28, -29, -30, -30]);
}

#[test]
fn test_heat_map11() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo//bar", &[], &mut v), &mut vec![83, -3, -4, -5, -6, 80, -6, -6]);
}

#[test]
fn test_heat_map12() {
    let mut v = Vec::new();
    assert_eq!(heat_map("foo//bar", &['/'], &mut v), &mut vec![41, -45, -46, -47, -46, 79, -7, -7]);
}

