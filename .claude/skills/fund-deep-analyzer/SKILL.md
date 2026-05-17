---
name: fund-deep-analyzer
description: 显式触发"单基金深度分析"工作流。当用户输入 `/fund-deep-analyzer <6位代码>` 时使用本技能。它会启动同名项目级 subagent，按 8 节模板对该基金做 23 字段研究级中文报告。**与自动触发 agent 的区别**：本 skill 是手动入口，确保用户意图明确（避免主 Claude 把"提个代码闲聊"误判成深度分析）。
---

# fund-deep-analyzer（手动触发版）

本 skill 是项目级 subagent `fund-deep-analyzer` 的显式入口。**所有报告生成逻辑都在 agent 里**，不在这份 skill 里重复定义。

## 用法

```
/fund-deep-analyzer 020262
/fund-deep-analyzer 001257
```

或者带追加要求：

```
/fund-deep-analyzer 020262 重点看费率和分红
/fund-deep-analyzer 110011 帮我对比 000171
```

## 实际行为

1. **参数为 6 位基金代码** → spawn `fund-deep-analyzer` agent，把代码 + 任何附加要求一起传给 agent。
2. **参数为多个代码（用空格/逗号分隔）** → 不走本 skill，改 spawn `fund-comparator` agent。
3. **参数不是 6 位代码** → 礼貌拒绝，提示正确用法。
4. **没参数** → 引导用户给代码。

## 输出契约

由 agent 决定。本 skill 不修改报告格式。

参考：`.claude/agents/fund-deep-analyzer.md`。

## 为什么同时有 agent 和 skill

| 入口 | 触发方式 | 适用 |
|------|---------|------|
| agent `fund-deep-analyzer` | **自动** — 主 Claude 根据 description 判断是否 spawn | 自然对话：「分析 020262」「这只怎么样」 |
| skill `/fund-deep-analyzer` | **手动** — 用户显式 slash command | 想强制走完整模板，避免主 Claude 误判（如把"提了个代码"当闲聊） |

两者底层调用同一个 agent，行为一致。
