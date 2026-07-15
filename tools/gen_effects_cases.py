#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""C 시리즈(글자효과·요약정보) 검증 파일 생성기 + 자체 검증 게이트.

gen_verification_set.sh 가 A/B 시리즈를 만든 뒤 이 스크립트를 한 번 호출한다.
CLI 에는 글자효과 플래그가 없으므로 **JSON IR 경유**로 만든다:
  1) 마크다운 → 우리 hwpx(base) 로 뼈대를 만들고,
  2) `hwp cat --format json` 으로 IR 을 떠서 python(stdlib) 으로 수술
     (char_shapes 에 효과 비트를 켠 문자모양을 추가하고 char_shape_runs 로 참조),
  3) `hwp new --from surgery.json` 으로 최종 파일을 방출한다.

각 파일은 다시 `hwp cat --format json` 으로 재읽기해 효과 비트/밑줄모양/번호형식/
metadata 가 실제로 살아있는지 단언한다. 하나라도 죽어 있으면 그 파일은 ❌ 처리한다.

표준 출력에는 파일당 한 줄(✅/❌ …)을 찍는다. bash 가 이 줄들을 REPORT 에 합치고
✅/❌ 개수로 pass/fail 을 집계한다. 종료코드: 실패가 있으면 1.

의존성: 파이썬 표준 라이브러리만 사용(json/subprocess/…). 외부 패키지 금지.
"""

import argparse
import copy
import datetime
import json
import os
import subprocess
import sys

# ── 글자모양 attr 비트(hwp5 스펙 = crates/hwp-model/src/header.rs 접근자와 동일) ──
ITALIC = 1 << 0
BOLD = 1 << 1
UL_BOTTOM = 1 << 2   # 밑줄 종류(bits2~3)=1 → 글자 아래
OUTLINE = 1 << 8     # 외곽선(bits8~10)
SHADOW = 1 << 11     # 그림자(bits11~12)
EMBOSS = 1 << 13     # 양각
ENGRAVE = 1 << 14    # 음각
SUPER = 1 << 15      # 위첨자
SUB = 1 << 16        # 아래첨자

# 밑줄 모양 코드(crates/hwpx read/write header line_type_code/name 과 대칭).
UL_DOT = 3           # 점선
UL_CIRCLE = 7        # 원형 점선(물결에 가장 가까운, 왕복되는 비실선)
UL_DOUBLE = 8        # 이중선

HWP = None  # 바이너리 경로(런타임 설정)
WORK = None  # 작업 디렉터리


def run_json(args):
    """hwp 서브커맨드를 실행하고 stdout(JSON)을 파싱해 돌려준다."""
    out = subprocess.run(
        [HWP, *args], capture_output=True, text=True
    )
    return out


def base_ir(md_text, tag):
    """마크다운 → base hwpx → IR(dict). 뼈대(폰트/테두리/스타일)를 얻기 위함."""
    md = os.path.join(WORK, f"{tag}.md")
    base = os.path.join(WORK, f"{tag}_base.hwpx")
    with open(md, "w", encoding="utf-8") as f:
        f.write(md_text)
    subprocess.run([HWP, "new", "--from", md, "-o", base],
                   capture_output=True, text=True)
    r = subprocess.run([HWP, "cat", base, "--format", "json"],
                       capture_output=True, text=True)
    return json.loads(r.stdout)


def emit(ir, out_path, tag):
    """IR(dict) → JSON 파일 → hwp new --from → out_path."""
    j = os.path.join(WORK, f"{tag}_ir.json")
    with open(j, "w", encoding="utf-8") as f:
        json.dump(ir, f, ensure_ascii=False)
    p = subprocess.run([HWP, "new", "--from", j, "-o", out_path],
                       capture_output=True, text=True)
    return p


def reread(path):
    r = subprocess.run([HWP, "cat", path, "--format", "json"],
                       capture_output=True, text=True)
    return json.loads(r.stdout)


def validate_ok(path):
    """hwpx 는 `hwp validate` 통과 여부를, 그 외는 재읽기 성공 여부를 본다."""
    r = subprocess.run([HWP, "validate", path],
                       capture_output=True, text=True)
    return "유효" in r.stdout, (r.stdout + r.stderr).strip().splitlines()[-1:] or [""]


def para_text(p):
    return "".join(c.get("Text", "") for c in p.get("chars", []) if isinstance(c, dict))


def find_para(ir, needle):
    """본문 문단 중 텍스트에 needle 이 든 첫 문단을 돌려준다."""
    for sec in ir["sections"]:
        for p in sec["paragraphs"]:
            if needle in para_text(p):
                return p
    raise RuntimeError(f"문단 못 찾음: {needle!r}")


def clone_shape(ir, base_id):
    """char_shapes[base_id]를 복제해 새 항목으로 추가하고 (id, dict)를 돌려준다."""
    cs = copy.deepcopy(ir["header"]["char_shapes"][base_id])
    ir["header"]["char_shapes"].append(cs)
    return len(ir["header"]["char_shapes"]) - 1, cs


def run_shapes(ir, p):
    """문단 p 의 char_shape_runs 가 참조하는 (pos, shape_dict) 목록."""
    return [(pos, ir["header"]["char_shapes"][sid]) for pos, sid in p["char_shape_runs"]]


def filetime(dt):
    """aware datetime → FILETIME raw u64(1601-01-01 UTC, 100ns)."""
    epoch = datetime.datetime(1601, 1, 1, tzinfo=datetime.timezone.utc)
    return int((dt - epoch).total_seconds() * 10_000_000)


# 2026-07-15 UTC 근처의 그럴듯한 작성/수정 일시.
CREATE_FT = filetime(datetime.datetime(2026, 7, 15, 9, 0, 0,
                                       tzinfo=datetime.timezone.utc))
MODIFY_FT = filetime(datetime.datetime(2026, 7, 15, 14, 30, 0,
                                       tzinfo=datetime.timezone.utc))

META = {
    "title": "실기 검증 요약정보 문서",
    "author": "홍길동",
    "subject": "글자효과 및 요약정보 검증",
    "keywords": "hwp, 실기검증, 요약정보",
    "description": "C 시리즈 요약정보 검증용 문서입니다.",
    "last_saved_by": "검증 담당자",
    "create_time": CREATE_FT,
    "modify_time": MODIFY_FT,
}


class Fail(Exception):
    pass


def whole(ir, p, shape_id):
    """문단 전체에 shape_id 를 적용."""
    p["char_shape_runs"] = [[0, shape_id]]


def partial(ir, p, base_id, effect_id, idx):
    """문단의 idx 번째 글자에만 effect_id, 나머지는 base_id."""
    p["char_shape_runs"] = [[0, base_id], [idx, effect_id], [idx + 1, base_id]]


# ── 개별 케이스 ────────────────────────────────────────────────────────────

def c1_shadow(dest):
    out = os.path.join(dest, "C1_그림자.hwpx")
    ir = base_ir("그림자 효과 텍스트\n", "c1")
    p = find_para(ir, "그림자 효과")
    base = p["char_shape_runs"][0][1]
    sid, cs = clone_shape(ir, base)
    cs["attr"] |= SHADOW
    cs["shadow_color"] = 0x808080         # 회색
    cs["shadow_gap"] = [5, 5]
    whole(ir, p, sid)
    emit(ir, out, "c1")

    ok, _ = validate_ok(out)
    if not ok:
        raise Fail("validate 실패")
    r = reread(out)
    tp = find_para(r, "그림자 효과")
    hit = [c for _, c in run_shapes(r, tp) if c["attr"] & SHADOW]
    if not hit:
        raise Fail("그림자 비트(bit11) 소실")
    if hit[0]["shadow_gap"] == [0, 0]:
        raise Fail("그림자 간격 소실")
    return out, "그림자(attr bit11)+shadow_color=#808080+gap(5,5)"


def c2_outline(dest):
    out = os.path.join(dest, "C2_외곽선.hwpx")
    ir = base_ir("외곽선 효과 텍스트\n", "c2")
    p = find_para(ir, "외곽선 효과")
    base = p["char_shape_runs"][0][1]
    sid, cs = clone_shape(ir, base)
    cs["attr"] |= OUTLINE
    whole(ir, p, sid)
    emit(ir, out, "c2")

    ok, _ = validate_ok(out)
    if not ok:
        raise Fail("validate 실패")
    r = reread(out)
    tp = find_para(r, "외곽선 효과")
    if not any(c["attr"] & OUTLINE for _, c in run_shapes(r, tp)):
        raise Fail("외곽선 비트(bit8) 소실")
    return out, "외곽선(attr bit8, SOLID)"


def c3_emboss_engrave(dest):
    out = os.path.join(dest, "C3_양각음각.hwpx")
    ir = base_ir("양각 효과 문단입니다\n\n음각 효과 문단입니다\n", "c3")
    pe = find_para(ir, "양각 효과")
    pg = find_para(ir, "음각 효과")
    se, cse = clone_shape(ir, pe["char_shape_runs"][0][1])
    cse["attr"] |= EMBOSS
    whole(ir, pe, se)
    sg, csg = clone_shape(ir, pg["char_shape_runs"][0][1])
    csg["attr"] |= ENGRAVE
    whole(ir, pg, sg)
    emit(ir, out, "c3")

    ok, _ = validate_ok(out)
    if not ok:
        raise Fail("validate 실패")
    r = reread(out)
    if not any(c["attr"] & EMBOSS for _, c in run_shapes(r, find_para(r, "양각 효과"))):
        raise Fail("양각 비트(bit13) 소실")
    if not any(c["attr"] & ENGRAVE for _, c in run_shapes(r, find_para(r, "음각 효과"))):
        raise Fail("음각 비트(bit14) 소실")
    return out, "양각(bit13) 문단 + 음각(bit14) 문단"


def c4_scripts(dest):
    out = os.path.join(dest, "C4_첨자.hwpx")
    # 첫 문단(제목)이 구역정의 컨트롤을 흡수하도록 앞에 제목을 둔다 → 첨자 문단은
    # 순수 텍스트가 되어 글자 인덱스 = WCHAR 위치가 일치한다(부분 구간 정확).
    ir = base_ir("# 첨자 검증\n\nx2의 2가 위첨자\n\nH2O의 2가 아래첨자\n", "c4")
    pu = find_para(ir, "위첨자")
    pd = find_para(ir, "아래첨자")
    # 각 문단에서 첫 '2' 글자에만 첨자(x2 / H2O 의 '2').
    iu = para_text(pu).index("2")
    bu = pu["char_shape_runs"][0][1]
    su, csu = clone_shape(ir, bu)
    csu["attr"] |= SUPER
    partial(ir, pu, bu, su, iu)
    idd = para_text(pd).index("2")
    bd = pd["char_shape_runs"][0][1]
    sd, csd = clone_shape(ir, bd)
    csd["attr"] |= SUB
    partial(ir, pd, bd, sd, idd)
    emit(ir, out, "c4")

    ok, _ = validate_ok(out)
    if not ok:
        raise Fail("validate 실패")
    r = reread(out)
    # 위첨자: idx 를 시작으로 하는 run 의 shape 에 bit15.
    tu = find_para(r, "위첨자")
    if not any(pos == iu and c["attr"] & SUPER for pos, c in run_shapes(r, tu)):
        raise Fail("위첨자 비트(bit15) 소실")
    td = find_para(r, "아래첨자")
    if not any(pos == idd and c["attr"] & SUB for pos, c in run_shapes(r, td)):
        raise Fail("아래첨자 비트(bit16) 소실")
    return out, "위첨자(bit15)·아래첨자(bit16) 부분 구간(각 문단의 '2' 글자)"


def c5_underline(dest):
    out = os.path.join(dest, "C5_밑줄모양.hwpx")
    ir = base_ir("점선 밑줄 문단\n\n이중 밑줄 문단\n\n물결 밑줄 문단\n", "c5")
    specs = [("점선 밑줄", UL_DOT), ("이중 밑줄", UL_DOUBLE), ("물결 밑줄", UL_CIRCLE)]
    for needle, shape in specs:
        p = find_para(ir, needle)
        sid, cs = clone_shape(ir, p["char_shape_runs"][0][1])
        cs["attr"] |= UL_BOTTOM
        cs["underline_shape"] = shape
        whole(ir, p, sid)
    emit(ir, out, "c5")

    ok, _ = validate_ok(out)
    if not ok:
        raise Fail("validate 실패")
    r = reread(out)
    for needle, shape in specs:
        tp = find_para(r, needle)
        hit = [c for _, c in run_shapes(r, tp)
               if (c["attr"] >> 2) & 3 == 1 and c["underline_shape"] == shape]
        if not hit:
            raise Fail(f"{needle} 밑줄모양({shape}) 소실")
    return out, "밑줄 종류=아래(kind1) + 모양 점선(3)/이중(8)/원형점선(7)"


def c6_numbering(dest):
    out = os.path.join(dest, "C6_번호형식.hwpx")
    ir = base_ir("첫째 조항 문단\n\n둘째 조항 문단\n\n셋째 조항 문단\n", "c6")
    # 사용자 번호형식: "제^1조." (^1=1수준 번호 자리), 시작번호 5.
    ir["header"]["numbering_levels"] = [[
        {"start": 5, "fmt": "Digit", "template": "제^1조."}
    ]]
    # 각 조항 문단 para_shape 에 번호 머리(NUMBER)와 numbering_id 를 표기(IR 의미 보존).
    for needle in ("첫째 조항", "둘째 조항", "셋째 조항"):
        p = find_para(ir, needle)
        src = p["para_shape"]
        np = copy.deepcopy(ir["header"]["para_shapes"][src])
        np["attr1"] = np.get("attr1", 0) | (2 << 23) | (1 << 25)  # head_type=NUMBER, level1
        np["numbering_id"] = 1
        nid = len(ir["header"]["para_shapes"])
        ir["header"]["para_shapes"].append(np)
        p["para_shape"] = nid
    emit(ir, out, "c6")

    ok, _ = validate_ok(out)
    if not ok:
        raise Fail("validate 실패")
    r = reread(out)
    lv = r["header"].get("numbering_levels") or [[]]
    if not lv or not lv[0]:
        raise Fail("번호 정의 소실")
    n0 = lv[0][0]
    if n0.get("template") != "제^1조." or n0.get("start") != 5:
        raise Fail(f"번호형식 소실: {n0}")
    # 재읽기에서 각 조항 문단의 heading 링크(head_type=NUMBER, numbering_id) 보존 확인.
    shapes = r["header"]["para_shapes"]
    for needle in ("첫째 조항", "둘째 조항", "셋째 조항"):
        p = find_para(r, needle)
        ps = shapes[p["para_shape"]]
        head_type = (ps.get("attr1", 0) >> 23) & 0x3
        if head_type != 2:
            raise Fail(f"heading 미표시({needle}): head_type={head_type}")
        if ps.get("numbering_id", 0) <= 0:
            raise Fail(f"heading numbering_id 소실({needle}): {ps.get('numbering_id')}")
    return out, "번호 정의 template='제^1조.' start=5 + 문단 heading 링크(NUMBER) 보존"


def c7_all(dest):
    out = os.path.join(dest, "C7_글자효과통합.hwpx")
    lines = [
        "그림자: 이 문단은 글자 그림자",
        "외곽선: 이 문단은 외곽선",
        "양각: 이 문단은 양각",
        "음각: 이 문단은 음각",
        "위첨자: x2의 2가 위첨자",
        "아래첨자: H2O의 2가 아래첨자",
        "밑줄: 이 문단은 이중 밑줄",
    ]
    ir = base_ir("\n\n".join(lines) + "\n", "c7")

    def w(needle, mut):
        p = find_para(ir, needle)
        sid, cs = clone_shape(ir, p["char_shape_runs"][0][1])
        mut(cs)
        whole(ir, p, sid)

    def sh(cs):
        cs["attr"] |= SHADOW
        cs["shadow_color"] = 0x808080
        cs["shadow_gap"] = [5, 5]
    w("그림자:", sh)
    w("외곽선:", lambda cs: cs.__setitem__("attr", cs["attr"] | OUTLINE))
    w("양각:", lambda cs: cs.__setitem__("attr", cs["attr"] | EMBOSS))
    w("음각:", lambda cs: cs.__setitem__("attr", cs["attr"] | ENGRAVE))

    def ul(cs):
        cs["attr"] |= UL_BOTTOM
        cs["underline_shape"] = UL_DOUBLE
    w("밑줄:", ul)
    # 첨자는 라벨 뒤 'x2'/'H2O'의 '2'만. 라벨 "위첨자: x2..." 에서 '2'는 index 5.
    for needle, bit in (("위첨자:", SUPER), ("아래첨자:", SUB)):
        p = find_para(ir, needle)
        txt = para_text(p)
        # 라벨 뒤 첫 숫자 '2'의 위치.
        idx = txt.index("2")
        base = p["char_shape_runs"][0][1]
        sid, cs = clone_shape(ir, base)
        cs["attr"] |= bit
        partial(ir, p, base, sid, idx)
    emit(ir, out, "c7")

    ok, _ = validate_ok(out)
    if not ok:
        raise Fail("validate 실패")
    r = reread(out)
    shapes = r["header"]["char_shapes"]
    have = {
        "그림자": any(c["attr"] & SHADOW for c in shapes),
        "외곽선": any(c["attr"] & OUTLINE for c in shapes),
        "양각": any(c["attr"] & EMBOSS for c in shapes),
        "음각": any(c["attr"] & ENGRAVE for c in shapes),
        "위첨자": any(c["attr"] & SUPER for c in shapes),
        "아래첨자": any(c["attr"] & SUB for c in shapes),
        "밑줄": any(((c["attr"] >> 2) & 3) == 1 and c["underline_shape"] != 1
                    for c in shapes),
    }
    missing = [k for k, v in have.items() if not v]
    if missing:
        raise Fail("통합 효과 소실: " + ", ".join(missing))
    return out, "7효과(그림자·외곽선·양각·음각·위첨자·아래첨자·이중밑줄) 문단별"


def c8_summary_hwp(dest):
    out = os.path.join(dest, "C8_요약정보.hwp")
    ir = base_ir("요약정보 검증 본문 문단입니다.\n", "c8")
    ir["metadata"] = dict(META)
    p = emit(ir, out, "c8")
    if not os.path.exists(out) or os.path.getsize(out) == 0:
        raise Fail("hwp 생성 실패: " + (p.stderr.strip()[-160:]))
    r = reread(out)
    m = r.get("metadata", {})
    for k, v in META.items():
        if m.get(k) != v:
            raise Fail(f"metadata.{k} 불일치: {m.get(k)!r} != {v!r}")
    return out, "요약정보 8필드(제목/지은이/주제/키워드/설명/최종저장자/작성·수정 FILETIME)"


def c9_summary_hwpx(dest):
    out = os.path.join(dest, "C9_요약정보.hwpx")
    ir = base_ir("요약정보 검증 본문 문단입니다.\n", "c9")
    ir["metadata"] = dict(META)
    emit(ir, out, "c9")
    ok, _ = validate_ok(out)
    if not ok:
        raise Fail("validate 실패")
    r = reread(out)
    m = r.get("metadata", {})
    # hwpx OPF(content.hpf)는 이제 정품 형식으로 8필드를 모두 방출한다:
    # creator/subject/keyword/description/lastsaveby meta + CreatedDate/ModifiedDate(ISO-8601).
    # 문자열 6필드는 그대로 왕복. FILETIME 2필드는 ISO 초 정밀도라 하위 100ns가 절사되므로
    # 기대값도 초 단위로 내림(FT_PER_SEC=10,000,000 배수)해 비교한다. META의 작성/수정 시각은
    # 이미 초 경계라 절사 후에도 값이 동일하다.
    FT_PER_SEC = 10_000_000
    for k in ("title", "author", "subject", "keywords", "description", "last_saved_by"):
        if m.get(k) != META[k]:
            raise Fail(f"metadata.{k} 불일치: {m.get(k)!r} != {META[k]!r}")
    for k in ("create_time", "modify_time"):
        want = (META[k] // FT_PER_SEC) * FT_PER_SEC  # 초 절사
        if m.get(k) != want:
            raise Fail(f"metadata.{k} 불일치(초 절사): {m.get(k)!r} != {want!r}")
    return out, "요약정보 8필드(제목/지은이/주제/키워드/설명/최종저장자/작성·수정 FILETIME; ISO 초 정밀)"


CASES = [
    ("C1_그림자.hwpx", c1_shadow),
    ("C2_외곽선.hwpx", c2_outline),
    ("C3_양각음각.hwpx", c3_emboss_engrave),
    ("C4_첨자.hwpx", c4_scripts),
    ("C5_밑줄모양.hwpx", c5_underline),
    ("C6_번호형식.hwpx", c6_numbering),
    ("C7_글자효과통합.hwpx", c7_all),
    ("C8_요약정보.hwp", c8_summary_hwp),
    ("C9_요약정보.hwpx", c9_summary_hwpx),
]


def main():
    global HWP, WORK
    ap = argparse.ArgumentParser()
    ap.add_argument("--hwp", required=True)
    ap.add_argument("--dest", required=True)
    ap.add_argument("--work", required=True)
    a = ap.parse_args()
    HWP, WORK = a.hwp, a.work
    os.makedirs(a.dest, exist_ok=True)
    os.makedirs(a.work, exist_ok=True)

    fails = 0
    for label, fn in CASES:
        try:
            _, detail = fn(a.dest)
            print(f"✅ {label} — {detail}")
        except Exception as e:  # noqa: BLE001 (검증 게이트: 어떤 실패든 ❌ 처리)
            fails += 1
            print(f"❌ {label} — {e}")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
