use crate::types::{TodoItem, TodoStatus};

pub fn create_rule_based_plan(objective: &str) -> (Vec<TodoItem>, Option<Vec<String>>) {
    let obj_lower = objective.to_lowercase();
    let mut todos = Vec::new();
    let mut criteria = Vec::new();

    let obj_trimmed = objective.trim();

    todos.push(TodoItem::new("T1", "分析项目结构和相关代码"));

    if contains_any(&obj_lower, &["测试", "test", "coverage", "覆盖"]) {
        todos.push(TodoItem::new("T2", "查看现有测试结构和测试框架"));
        todos.push(TodoItem::new("T3", "为核心模块编写单元测试"));
        todos.push(TodoItem::new("T4", "运行测试并修复失败"));
        todos.push(TodoItem::new("T5", "运行cargo clippy检查"));
        todos.push(TodoItem::new("T6", "验证测试覆盖率"));
        criteria.push("所有新增测试通过".to_string());
        criteria.push("cargo check 无错误".to_string());
    } else if contains_any(&obj_lower, &["重构", "refactor", "迁移", "migrate", "重写", "rewrite"]) {
        todos.push(TodoItem::new("T2", "制定重构计划并确认需要修改的模块"));
        todos.push(TodoItem::new("T3", "按模块逐步重构"));
        todos.push(TodoItem::new("T4", "重构后运行cargo check验证"));
        todos.push(TodoItem::new("T5", "运行现有测试确保功能正常"));
        todos.push(TodoItem::new("T6", "cargo clippy检查"));
        criteria.push("cargo check 0 error".to_string());
        criteria.push("所有现有测试通过".to_string());
    } else if contains_any(&obj_lower, &["修复", "fix", "bug", "错误", "报错"]) {
        todos.push(TodoItem::new("T2", "复现问题并定位错误位置"));
        todos.push(TodoItem::new("T3", "分析错误根因"));
        todos.push(TodoItem::new("T4", "实施修复"));
        todos.push(TodoItem::new("T5", "验证修复（cargo check + 相关测试）"));
        criteria.push("错误不再出现".to_string());
        criteria.push("cargo check 无错误".to_string());
    } else if contains_any(&obj_lower, &["功能", "feature", "新增", "添加", "add", "实现", "implement"]) {
        todos.push(TodoItem::new("T2", "分析现有代码结构，确定新增代码位置"));
        todos.push(TodoItem::new("T3", "实现核心功能逻辑"));
        todos.push(TodoItem::new("T4", "集成到现有模块中"));
        todos.push(TodoItem::new("T5", "cargo check 验证编译"));
        todos.push(TodoItem::new("T6", "编写/运行测试验证功能"));
        criteria.push("功能按预期工作".to_string());
        criteria.push("cargo check 0 error".to_string());
    } else {
        todos.push(TodoItem::new("T2", "分析代码库，理解当前状态"));
        todos.push(TodoItem::new("T3", "按目标描述逐步实现修改"));
        todos.push(TodoItem::new("T4", "cargo check 验证编译"));
        todos.push(TodoItem::new("T5", "运行测试验证"));
        criteria.push("cargo check 无错误".to_string());
    }

    todos.push(TodoItem::new(
        format!("T{}", todos.len() + 1),
        "最终验证：cargo check + 测试，总结成果",
    ));

    if !todos.is_empty() {
        if let Some(t) = todos.get_mut(1) {
            t.status = TodoStatus::InProgress;
        }
    }

    criteria.push(format!("目标达成：{}", obj_trimmed));
    (todos, Some(criteria))
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|k| text.contains(k))
}
