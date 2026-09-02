// audience: internal
// # personal-service-remote-headers
//
// 该文件验证远端 HTTP 转发使用的头名称和值. 通过验证的内容不包含控制字符.

// //// 验证 HTTP 头名称和值 [@x380kkm 2026-07-23] ////
pub(super) fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

pub(super) fn is_value(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte == b'\t' || !byte.is_ascii_control())
}
// //// /验证 HTTP 头名称和值 ////
