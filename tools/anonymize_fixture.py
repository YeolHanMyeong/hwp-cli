#!/usr/bin/env python3
"""fixtures/samples/report-tables.hwpx 본문 익명화 — 내용을 예시 문구로 재작성.

구조(문단/표/병합/스타일/실행 서식/네임스페이스 선언)는 원문 바이트 그대로 두고
<hp:t> 텍스트만 수술적으로 교체한다(XML 재직렬화 없음 — 한글은 루트 네임스페이스
선언 등 바이트 수준 형식을 엄격하게 본다). 문서 번호(3-2-1.)·불릿 마커(❍/❶/ - /①)와
가명 대학명(한빛대/미륵대/다온대/가온)은 보존한다. Preview/PrvImage.png는 원문
렌더라 제거하고(한글이 열 때 재생성), PrvText.txt는 새 본문으로 재생성한다.

텍스트를 바꾸면 원문의 줄 배치 캐시(hp:linesegarray)가 내용과 어긋나 한글이
"손상/변조" 보안 경고를 띄우므로, 본문 linesegarray는 제거해 한글이 재계산하게
한다(도형 내 텍스트 hp:drawText 내부는 writer 규칙대로 보존 — 04-hwpx-owpml §3.5).
Contents/content.hpf의 creator/lastsaveby도 hwp-cli로 중화한다.

사용: python3 tools/anonymize_fixture.py <in.hwpx> <out.hwpx>
"""
import re
import sys
import zipfile

# 이미 가명이라 보존하는 문자열(이 중 하나가 들어 있으면 이름만 새 문장에 심는다).
KEEP = ('한빛대', '미륵대', '다온대', '가온')

TOKEN = re.compile(r'<hp:p\b[^>]*>|</hp:p>|<hp:t\s*/>|<hp:t>.*?</hp:t>', re.S)
LINESEG = re.compile(r'<hp:linesegarray>.*?</hp:linesegarray>|<hp:linesegarray\s*/>', re.S)


def strip_body_linesegs(xml_text: str) -> str:
    """본문 linesegarray 제거(한글이 재계산). hp:drawText 내부는 보존."""
    out = []
    rest = xml_text
    while True:
        s = rest.find('<hp:drawText')
        if s < 0:
            out.append(LINESEG.sub('', rest))
            return ''.join(out)
        e = rest.find('</hp:drawText>', s)
        if e < 0:
            raise SystemExit('hp:drawText 닫힘 태그 없음 — 입력 확인 필요')
        e += len('</hp:drawText>')
        out.append(LINESEG.sub('', rest[:s]))
        out.append(rest[s:e])  # 도형 내 텍스트: 줄 배치 보존
        rest = rest[e:]


def xml_unescape(s: str) -> str:
    return s.replace('&lt;', '<').replace('&gt;', '>').replace('&amp;', '&')


def xml_escape(s: str) -> str:
    return s.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')


def synth_text(old: str, idx: int) -> str:
    """문단 텍스트를 구조 마커는 유지하며 예시 문구로 변환한다."""
    t = old.strip()
    if not t:
        return old
    # 가명 대학명이 든 문단: 내용은 바꾸되 이름은 새 문장에 심는다(치환 테스트용).
    kept = [k for k in KEEP if k in t]
    if kept:
        name = kept[0] + ('학교' if kept[0].endswith('대') else '')
        if len(t) <= 40:
            return f'{name} 예시 교과목'
        m = re.match(r'^(\([^)]{1,12}\))\s*', t)
        label = m.group(1) + ' ' if m else ''
        return f'{label}예시 문장 {idx}입니다. {name} 관련 가상 내용으로, 실제 기관·사업과 무관합니다.'
    # 문서 번호 제목: "3-2-1. ..." → 접두 유지
    m = re.match(r'^(\d+(?:-\d+)+\.\s*)(\S)', t)
    if m:
        return f'{m.group(1)}예시 과제 제목 {idx}'
    # (괄호) 라벨 문단: 라벨 유지 + 길이 비례 예시 문장
    m = re.match(r'^(\([^)]{1,12}\))\s*', t)
    if m:
        return f'{m.group(1)} {filler(idx, len(t))}'
    # 마커 계열(❍ ❶-❾ ①-⑳ ㅇ ※ △ ○ -) + 공백: 마커 유지
    m = re.match(r'^([❍❶-❾①-⑳ㅇ※△○]*\s*[-–]?\s+)', t)
    if m and m.group(1).strip():
        return f'{m.group(1)}예시 항목 {idx} — {filler(idx, len(t))}'
    # 짧은 셀 텍스트(표 헤더/라벨)
    if len(t) <= 12:
        return f'예시{idx}'
    return filler(idx, len(t))


def filler(idx: int, approx: int) -> str:
    """원문 길이에 비례한 예시 문장(레이아웃/쪽수 근사 유지용)."""
    unit = f'예시 문장 {idx}입니다. 이 문서는 표·문단 편집 기능 테스트용 가상 내용으로, 실제 사업·기관과 무관합니다. '
    n = max(1, approx // len(unit) + 1)
    return (unit * n).strip()


def scan_paragraphs(xml_text: str):
    """<hp:t> 토큰을 소유 문단(innermost <hp:p>)별로 묶는다.

    반환: [(start,end,para_no,run_no_in_para,inner_text), ...] — <hp:t>…</hp:t> 토큰만.
    """
    paras = []  # 현재 열린 문단 스택 [(para_no, run_count)]
    runs = []
    para_no = -1
    for m in TOKEN.finditer(xml_text):
        tok = m.group(0)
        if tok.startswith('<hp:p'):
            if tok.endswith('/>'):
                continue
            para_no += 1
            paras.append([para_no, 0])
        elif tok == '</hp:p>':
            paras.pop()
        elif tok.startswith('<hp:t/>') or tok.startswith('<hp:t />'):
            continue  # 빈 run은 텍스트 없음 — 건드리지 않음
        else:  # <hp:t>...</hp:t>
            if not paras:
                continue
            entry = paras[-1]
            entry[1] += 1
            inner = tok[6:-7]
            runs.append((m.start(), m.end(), entry[0], entry[1], xml_unescape(inner)))
    return runs


def transform_section(xml_text: str):
    runs = scan_paragraphs(xml_text)
    # 문단별 전체 텍스트 → 새 텍스트 결정
    para_texts = {}
    for (_, _, pno, _, inner) in runs:
        para_texts[pno] = para_texts.get(pno, '') + inner
    new_text = {}
    for idx, pno in enumerate(sorted(para_texts), start=1):
        old = para_texts[pno]
        new_text[pno] = synth_text(old, idx) if old.strip() else old

    out = []
    pos = 0
    plain = []
    for (start, end, pno, rno, inner) in runs:
        out.append(xml_text[pos:start])
        pos = end
        if rno == 1:
            out.append('<hp:t>' + xml_escape(new_text[pno]) + '</hp:t>')
            plain.append(new_text[pno])
        else:
            out.append('<hp:t/>')
    out.append(xml_text[pos:])
    return ''.join(out), plain


def main() -> None:
    src, dst = sys.argv[1], sys.argv[2]
    zin = zipfile.ZipFile(src)
    patched = {}
    plain_parts = []
    for name in zin.namelist():
        if name.startswith('Contents/section') and name.endswith('.xml'):
            body, plain = transform_section(zin.read(name).decode('utf-8'))
            patched[name] = strip_body_linesegs(body).encode('utf-8')
            plain_parts.extend(plain)
        elif name == 'Contents/content.hpf':
            hpf = zin.read(name).decode('utf-8')
            hpf = re.sub(
                r'(name="(?:creator|lastsaveby)" content="text">)[^<]*', r'\g<1>hwp-cli', hpf
            )
            patched[name] = hpf.encode('utf-8')

    zout = zipfile.ZipFile(dst, 'w', zipfile.ZIP_DEFLATED)
    for item in zin.infolist():
        if item.filename == 'Preview/PrvImage.png':
            continue  # 원문 렌더 이미지 — 제거(한글이 열 때 재생성)
        if item.filename in patched:
            zout.writestr(item, patched[item.filename])
        elif item.filename == 'Preview/PrvText.txt':
            preview = '\n'.join(plain_parts)[:4000]
            zout.writestr(item, preview.encode('utf-8'))
        else:
            zout.writestr(item, zin.read(item.filename))
    zout.close()
    print(f'{src} → {dst}: 문단 텍스트 변환 완료')


if __name__ == '__main__':
    main()
