#!/usr/bin/env python3
"""fixtures/samples/report-tables.hwpx 본문 익명화 — 내용을 예시 문구로 재작성.

구조(문단/표/병합/스타일/실행 서식)는 그대로 두고 <hp:t> 텍스트만 규칙 기반 예시
문구로 바꾼다. 문서 번호(3-2-1.)·불릿 마커(❍/❶/ - /①)와 이미 가명인 대학명
(한빛대/미륵대/다온대)은 보존한다. Preview/PrvImage.png는 원문 렌더라 제거하고
(한글이 열 때 재생성), PrvText.txt는 새 본문으로 재생성한다.

사용: python3 tools/anonymize_fixture.py <in.hwpx> <out.hwpx>
"""
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

HP = 'http://www.hancom.co.kr/hwpml/2011/paragraph'

# 이미 가명이라 보존하는 문자열(이 중 하나가 들어 있으면 그 문단은 건드리지 않음).
KEEP = ('한빛대', '미륵대', '다온대', '가온')


def register_namespaces(xml_text: str) -> None:
    """루트 xmlns 선언을 전부 등록해 재직렬화 시 접두사를 보존한다."""
    for prefix, uri in re.findall(r'xmlns:([A-Za-z0-9]+)="([^"]+)"', xml_text):
        ET.register_namespace(prefix, uri)


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
    m = re.match(r'^([❍❶-❾①-⑳ㅇ※△○<>]*\s*[-–]?\s+)', t)
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


def para_text(p: ET.Element) -> str:
    # 문단 직계 run의 텍스트만 — p.iter는 중첩 표 난 run까지 잡아 오염된다.
    return ''.join(t.text or '' for t in p.findall(f'./{{{HP}}}run/{{{HP}}}t'))


def main() -> None:
    src, dst = sys.argv[1], sys.argv[2]
    zin = zipfile.ZipFile(src)
    names = zin.namelist()

    # 1. section*.xml 본문 변환
    plain_parts = []
    patched = {}
    for name in names:
        if not (name.startswith('Contents/section') and name.endswith('.xml')):
            continue
        xml_text = zin.read(name).decode('utf-8')
        register_namespaces(xml_text)
        root = ET.fromstring(xml_text)
        idx = 0
        for p in root.iter(f'{{{HP}}}p'):
            runs = p.findall(f'./{{{HP}}}run/{{{HP}}}t')
            old = para_text(p)
            if not runs or not old.strip():
                continue
            idx += 1
            new = synth_text(old, idx)
            runs[0].text = new
            for r in runs[1:]:
                r.text = ''
            plain_parts.append(new)
        body = ET.tostring(root, encoding='unicode')
        patched[name] = (
            '<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>' + body
        ).encode('utf-8')

    # 2. ZIP 재작성: Preview/PrvImage.png 제거, PrvText.txt 재생성, 나머지 복사
    zout = zipfile.ZipFile(dst, 'w', zipfile.ZIP_DEFLATED)
    for item in zin.infolist():
        if item.filename == 'Preview/PrvImage.png':
            continue
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
