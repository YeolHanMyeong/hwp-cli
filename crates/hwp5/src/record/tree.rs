//! 평면 레코드 스트림 ↔ 레벨 기반 트리.
//!
//! 잘 형성된 파일에서 레코드의 level == 트리 깊이이므로,
//! 트리로 복원했다가 깊이로 level을 재계산해 직렬화하면
//! 압축 해제 스트림 기준 바이트 동일 왕복이 성립한다.

use crate::codec::ByteWriter;
use crate::record::header::RecordHeader;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordNode {
    /// 원시 태그 — enum 변환은 상위 계층의 선택.
    pub tag: u16,
    pub data: Vec<u8>,
    pub children: Vec<RecordNode>,
}

impl RecordNode {
    /// 평면 (header, data) 목록을 level 기반으로 트리로 복원한다.
    ///
    /// level이 비단조적으로 튀는(부모 없이 깊어지는) 레코드는 가장
    /// 가까운 조상에 붙이고 경고를 누적한다 — 손상 관용 모드.
    pub fn build_forest(flat: Vec<(RecordHeader, Vec<u8>)>) -> (Vec<RecordNode>, Vec<String>) {
        let mut warnings = Vec::new();
        let mut roots: Vec<RecordNode> = Vec::new();
        // stack[i]는 깊이 i에 열려 있는 노드
        let mut stack: Vec<RecordNode> = Vec::new();

        for (idx, (hdr, data)) in flat.into_iter().enumerate() {
            let mut level = usize::from(hdr.level);
            if level > stack.len() {
                warnings.push(format!(
                    "레코드 #{idx}(tag 0x{:03X}): level {level}이 현재 깊이 {}를 초과 — 가장 가까운 조상에 연결",
                    hdr.tag,
                    stack.len(),
                ));
                level = stack.len();
            }
            // 현재 레벨보다 깊게 열린 노드들을 닫는다
            while stack.len() > level {
                let done = stack.pop().expect("stack.len() > level >= 0");
                Self::attach(done, &mut stack, &mut roots);
            }
            stack.push(RecordNode {
                tag: hdr.tag,
                data,
                children: Vec::new(),
            });
        }
        while let Some(done) = stack.pop() {
            Self::attach(done, &mut stack, &mut roots);
        }
        (roots, warnings)
    }

    fn attach(done: RecordNode, stack: &mut [RecordNode], roots: &mut Vec<RecordNode>) {
        match stack.last_mut() {
            Some(parent) => parent.children.push(done),
            None => roots.push(done),
        }
    }

    /// 트리를 평면 레코드 스트림으로 재직렬화한다 (level = 깊이).
    pub fn serialize_forest(roots: &[RecordNode]) -> Vec<u8> {
        let mut w = ByteWriter::new();
        for node in roots {
            node.serialize_into(&mut w, 0);
        }
        w.into_bytes()
    }

    fn serialize_into(&self, w: &mut ByteWriter, depth: u16) {
        RecordHeader {
            tag: self.tag,
            level: depth,
            size: self.data.len() as u32,
        }
        .encode(w);
        w.write_bytes(&self.data);
        for child in &self.children {
            child.serialize_into(w, depth + 1);
        }
    }

    /// 자신을 포함한 전체 노드 수.
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(RecordNode::count).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(tag: u16, level: u16, data: &[u8]) -> (RecordHeader, Vec<u8>) {
        (
            RecordHeader {
                tag,
                level,
                size: data.len() as u32,
            },
            data.to_vec(),
        )
    }

    #[test]
    fn 트리_복원() {
        // A(0) ─ B(1) ─ C(2), D(1) / E(0)
        let flat = vec![
            rec(1, 0, b"A"),
            rec(2, 1, b"B"),
            rec(3, 2, b"C"),
            rec(4, 1, b"D"),
            rec(5, 0, b"E"),
        ];
        let (roots, warnings) = RecordNode::build_forest(flat);
        assert!(warnings.is_empty());
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].children.len(), 2);
        assert_eq!(roots[0].children[0].children.len(), 1);
        assert_eq!(roots[1].children.len(), 0);
    }

    #[test]
    fn 직렬화_왕복() {
        let flat = vec![
            rec(1, 0, b"A"),
            rec(2, 1, b"B"),
            rec(3, 2, b"C"),
            rec(5, 0, b"E"),
        ];
        let (roots, _) = RecordNode::build_forest(flat.clone());
        let bytes = RecordNode::serialize_forest(&roots);

        // 재스캔해서 같은 트리가 나와야 한다
        let rescanned = crate::record::scan::scan_stream(&bytes, crate::record::ScanMode::Strict)
            .expect("스캔 성공");
        assert_eq!(rescanned.roots, roots);
    }

    #[test]
    fn 레벨_점프는_관용_처리() {
        // level 0 다음에 갑자기 level 5
        let flat = vec![rec(1, 0, b"A"), rec(2, 5, b"B")];
        let (roots, warnings) = RecordNode::build_forest(flat);
        assert_eq!(warnings.len(), 1);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].children.len(), 1); // 가장 가까운 조상(A)에 연결
    }
}
