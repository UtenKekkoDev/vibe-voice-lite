use crate::state::InputMode;

pub const QUESTION: &str = "想切换到哪个模式？第一，语音听写；第二，开发指令；第三，需求问答。";
pub const RETRY: &str = "请说第一、第二、第三，或直接说模式名称。也可以说取消。";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeCommand {
    Ask,
    Choose(InputMode),
    Cancel,
    Retry,
}

/// Only complete utterances are commands. A mode name is ordinary dictation
/// until the user has opened the picker.
pub fn classify(transcript: &str, choosing: bool) -> Option<ModeCommand> {
    let command: String = transcript
        .trim_matches(|c: char| c.is_whitespace() || "，。！？、,.!?".contains(c))
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if matches!(
        command.as_str(),
        "切换模式" | "切換模式" | "switchmode" | "switchtype"
    ) {
        return Some(ModeCommand::Ask);
    }
    let selection = ["切换到", "切换成", "切换"]
        .iter()
        .find_map(|p| command.strip_prefix(p));
    let number = selection.unwrap_or(&command);
    // A bare cardinal ("二") may be dictated content. An ordinal ("第二")
    // is a shortcut even before opening the picker.
    if choosing || selection.is_some() || number.starts_with('第') {
        let number = number.strip_suffix("模式").unwrap_or(number);
        let number = number.strip_suffix("个").unwrap_or(number);
        let number = number.strip_prefix('第').unwrap_or(number);
        let mode = match number {
            "一" | "1" | "１" => Some(InputMode::DirectPrompt),
            "二" | "两" | "2" | "２" => Some(InputMode::CodeEdit),
            "三" | "3" | "３" => Some(InputMode::Interview),
            _ => None,
        };
        if let Some(mode) = mode {
            return Some(ModeCommand::Choose(mode));
        }
    }
    if !choosing {
        return None;
    }
    if matches!(
        command.as_str(),
        "取消" | "取消切换" | "取消切換" | "不切换了" | "cancel"
    ) {
        return Some(ModeCommand::Cancel);
    }
    let name = command
        .strip_prefix("切换到")
        .or_else(|| command.strip_prefix("切换成"))
        .unwrap_or(&command);
    let name = name.strip_suffix("模式").unwrap_or(name);
    let mode = match name {
        "语音听写" | "語音聽寫" | "听写" | "普通" | "普通口述" | "dictation" => {
            InputMode::DirectPrompt
        }
        "开发指令" | "開發指令" | "开发" | "代码" | "代码整理" | "code" | "codemode" => {
            InputMode::CodeEdit
        }
        "需求问答" | "需求問答" | "问答" | "访谈" | "需求访谈" | "interview" => {
            InputMode::Interview
        }
        _ => return Some(ModeCommand::Retry),
    };
    Some(ModeCommand::Choose(mode))
}

/// ASR can omit the comma before a spoken command. Treat bare trailing
/// directives as commands, except in common descriptions of an input action.
/// Return a slice so the original text and casing survive delivery.
pub enum Verbatim<'a> {
    Previous,
    Inline(&'a str),
}

pub fn verbatim(transcript: &str) -> Option<Verbatim<'_>> {
    let boundary = |c: char| "，,。.!！?？；;：:".contains(c);
    let text = transcript.trim_end_matches(|c: char| c.is_whitespace() || boundary(c));
    let text = text
        .trim_end_matches(['呃', '嗯', '啊', '吧', '哈'])
        .trim_end_matches(|c: char| c.is_whitespace() || boundary(c));
    let standalone = text
        .trim_start_matches(['嗯', '呃'])
        .trim_start_matches(boundary)
        .trim();
    if matches!(
        standalone,
        "帮我直接输入"
            | "请直接输入"
            | "就直接输入"
            | "直接发出来"
            | "直接把原文发出来"
            | "把刚才那段原话直接发出来"
            | "把刚才那句直接输入"
    ) {
        return Some(Verbatim::Previous);
    }
    for suffix in [
        "这句话直接输入",
        "这段话直接输入",
        "这句直接输入",
        "这段直接输入",
        "原文直接输入",
        "原文直接输出",
        "按原文输出",
        "直接直出",
        "直接输出",
        "直接输入",
        "原文直出",
        "直接发原文",
        "直出",
    ] {
        let Some(body) = text.strip_suffix(suffix) else {
            continue;
        };
        if standalone == suffix {
            return Some(Verbatim::Previous);
        }
        let body = body.trim_end();
        if !body.ends_with(boundary)
            && [
                "要",
                "能",
                "可以",
                "允许",
                "支持",
                "禁止",
                "不能",
                "无需",
                "不用",
                "不要",
                "不",
                "无法",
                "应该",
                "需要",
                "请",
                "叫",
                "叫做",
                "改为",
                "改成",
                "选择",
                "用户",
                "怎么",
                "如何",
                "不想",
                "想",
                "而是",
                "是",
                "通过",
                "尝试",
                "必须",
                "不要把",
                "别把",
                "不能把",
                "默认",
                "别",
            ]
            .iter()
            .any(|ending| body.ends_with(ending))
        {
            return None;
        }
        // Keep sentence-ending punctuation but drop the command's separator.
        let body = body
            .trim_end_matches(['，', ',', '：', ':', ';', '；'])
            .trim();
        if !body.is_empty() {
            return Some(Verbatim::Inline(body));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_standalone_requests_open_the_picker() {
        for text in [
            "切换模式。",
            " SWITCH Mode! ",
            "Switch type.",
            "切 換 模 式",
        ] {
            assert_eq!(classify(text, false), Some(ModeCommand::Ask));
        }
        for text in [
            "帮我切换模式",
            "请把 switch mode 翻译成中文",
            "开发指令",
            "取消",
        ] {
            assert_eq!(classify(text, false), None);
        }
    }

    #[test]
    fn spoken_numbers_route_only_complete_selections() {
        for choosing in [false, true] {
            for (text, mode) in [
                ("切换第一个。", InputMode::DirectPrompt),
                ("第1", InputMode::DirectPrompt),
                ("第二", InputMode::CodeEdit),
                ("切换到第 2 个模式", InputMode::CodeEdit),
                ("切换第三个", InputMode::Interview),
                ("第三。", InputMode::Interview),
            ] {
                assert_eq!(classify(text, choosing), Some(ModeCommand::Choose(mode)));
            }
        }
        assert_eq!(classify("2", false), None);
        assert_eq!(
            classify("二", true),
            Some(ModeCommand::Choose(InputMode::CodeEdit))
        );
        assert_eq!(classify("第二个按钮", false), None);
        assert_eq!(classify("第四", true), Some(ModeCommand::Retry));
    }

    #[test]
    fn verbatim_is_a_command_clause_not_a_keyword() {
        for command in [
            "直接输入。",
            "直接直出，呃。",
            "原文直出",
            "帮我直接输入",
            "嗯，直接输入。",
            "原文直接输入。",
        ] {
            assert!(matches!(verbatim(command), Some(Verbatim::Previous)));
        }
        for sentence in [
            "表单允许直接输入",
            "按钮叫“直接输入”",
            "不要直接输入",
            "不要把这句直接输入",
            "输入框默认直接输入",
            "直接输入后可以保存",
        ] {
            assert!(verbatim(sentence).is_none());
        }
        for (sentence, expected) in [
            ("保留 Foo  Bar，直接输入。", "保留 Foo  Bar"),
            ("明天下午3点开会直接输入。", "明天下午3点开会"),
            ("收到了。这句直接输入", "收到了。"),
            ("明天下午开会这句直接输入。", "明天下午开会"),
        ] {
            assert!(matches!(verbatim(sentence), Some(Verbatim::Inline(s)) if s == expected));
        }
    }

    #[test]
    fn choices_accept_display_names_and_previous_names() {
        for mode in [
            InputMode::DirectPrompt,
            InputMode::CodeEdit,
            InputMode::Interview,
        ] {
            assert_eq!(
                classify(mode.label(), true),
                Some(ModeCommand::Choose(mode))
            );
        }
        assert_eq!(
            classify("切换到代码模式。", true),
            Some(ModeCommand::Choose(InputMode::CodeEdit))
        );
        assert_eq!(
            classify("普通模式", true),
            Some(ModeCommand::Choose(InputMode::DirectPrompt))
        );
        assert_eq!(
            classify("需求访谈", true),
            Some(ModeCommand::Choose(InputMode::Interview))
        );
        assert_eq!(classify("取消切换", true), Some(ModeCommand::Cancel));
        assert_eq!(classify("换个我还没想好的", true), Some(ModeCommand::Retry));
    }
}
