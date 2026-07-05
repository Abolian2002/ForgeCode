# SWE-bench 评测 mini-code 流程

## 一、核心结论

用 SWE-bench 测 mini-code 时，要分成两个阶段：

```text
阶段 1：Agent 运行阶段
mini-code 读取任务、修改代码、生成 patch

阶段 2：官方评测阶段
SWE-bench harness 应用 patch、跑测试、统计结果
```

官方预构建镜像主要优化的是第二阶段，也就是评测阶段。它可以帮你自动拉镜像、应用 patch、跑测试，但不会替你的 mini-code 生成 patch。

---

## 二、整体流程

```text
读取 SWE-bench Verified 任务
    ↓
Adapter 调用 mini-code 解题
    ↓
mini-code 在仓库中修改代码
    ↓
导出 git diff
    ↓
生成 predictions.jsonl
    ↓
SWE-bench harness 读取 predictions.jsonl
    ↓
拉取官方预构建镜像
    ↓
应用 patch 并运行测试
    ↓
输出 resolved rate 和评测报告
```

---

## 三、阶段 1：Agent 运行阶段

这一阶段是 mini-code 自己完成的，官方 harness 不会替你做。

流程如下：

```text
读取 SWE-bench instance
    ↓
checkout 对应仓库和 base commit
    ↓
把 issue 描述交给 mini-code
    ↓
mini-code 搜索、阅读、修改、运行验证
    ↓
导出 git diff
    ↓
保存为 predictions.jsonl
```

这一阶段的目标是：

- 让 mini-code 在真实仓库里解决 issue
- 生成最终代码改动
- 把代码改动保存成 SWE-bench 能识别的 patch 格式

---

## 四、Adapter 的作用

SWE-bench 官方不知道你的 mini-code 怎么调用，所以需要一个 Adapter。

Adapter 可以理解为 mini-code 和 SWE-bench 之间的桥梁。

它负责：

```text
1. 读取 SWE-bench 数据集中的任务
2. 为每个任务准备工作目录
3. 把 issue 描述包装成 prompt
4. 调用 mini-code 执行任务
5. 等待 mini-code 完成或超时
6. 收集 git diff
7. 写入 predictions.jsonl
```

Adapter 的输入是：

```text
SWE-bench instance
```

Adapter 的输出是：

```text
predictions.jsonl
```

示例结构：

```json
{
  "instance_id": "django__django-12345",
  "model_name_or_path": "mini-code",
  "model_patch": "diff --git a/..."
}
```

---

## 五、mini-code 的输入 Prompt

Adapter 可以把 SWE-bench 的 issue 包装成一个更适合 Agent 的任务描述。

示例：

```text
你正在解决一个真实 GitHub issue。

要求：
1. 阅读 issue 描述，定位问题根因
2. 修改代码，生成最小必要修复
3. 尽量运行相关测试或检查命令
4. 不要做无关重构
5. 最终保持 git diff 只包含必要改动

Issue:
<problem_statement>
```

如果要对比不同模式，可以分别生成：

```text
普通模式：mini-code <issue>
/goal 模式：mini-code "/goal 修复以下 issue，并确保测试通过：..."
/team 模式：mini-code "/team 修复以下 issue，并进行测试和审查：..."
```

---

## 六、阶段 2：官方评测阶段

这一阶段由 SWE-bench harness 完成。

流程如下：

```text
读取 predictions.jsonl
    ↓
拉取或构建 Docker 镜像
    ↓
checkout 对应 base commit
    ↓
应用 mini-code 生成的 patch
    ↓
运行官方测试
    ↓
判断 resolved / unresolved
    ↓
输出评测报告
```

如果使用官方预构建镜像，harness 会尽量直接拉取已经准备好的环境，避免本地为每个任务重新构建 Docker 镜像。

这能节省大量评测时间，但前提仍然是你已经有了 `predictions.jsonl`。

---

## 七、评测命令示例

生成好 `predictions.jsonl` 后，可以用类似命令运行评测：

```bash
python -m swebench.harness.run_evaluation \
  --dataset_name princeton-nlp/SWE-bench_Verified \
  --split test \
  --predictions_path predictions.jsonl \
  --max_workers 8 \
  --run_id minicode_verified \
  --use_prebuilt_images true
```

实际参数可能因 SWE-bench 版本不同略有差异，运行前建议先看：

```bash
python -m swebench.harness.run_evaluation --help
```

---

## 八、SWE-bench 如何判断成功

SWE-bench 不看 mini-code 的解释文本，只看 patch 应用后的测试结果。

主要看两类测试：

```text
FAIL_TO_PASS：原本失败，修复后应该通过
PASS_TO_PASS：原本通过，修复后仍应通过
```

一个任务通常需要满足：

```text
FAIL_TO_PASS 通过
PASS_TO_PASS 不被破坏
```

才算 resolved。

最终核心指标是：

```text
Resolved Rate = 成功修复任务数 / 总任务数
```

---

## 九、推荐执行顺序

不要一开始直接跑 Verified 500 个任务，建议分阶段：

```text
1. 先跑 1 个任务
   验证 Adapter、mini-code 调用、patch 导出、官方评测都能跑通

2. 再跑 5-10 个任务
   检查超时、空 patch、patch 无法应用、评测失败等问题

3. 再跑 30-50 个任务
   初步比较普通模式、/goal、/team 的效果

4. 最后跑 SWE-bench Verified 500 个任务
   得到正式 resolved rate
```

---

## 十、如果要比较 `/goal` 和 `/team`

可以分别生成三份预测文件：

```text
preds_baseline.jsonl
preds_goal.jsonl
preds_team.jsonl
```

然后分别运行评测：

```text
baseline resolved rate
/goal resolved rate
/team resolved rate
```

这样才能证明 `/goal`、`/team` 是否真的提升了 SWE-bench 表现。

---

## 十一、注意事项

- 官方预构建镜像加速的是评测阶段，不是 Agent 解题阶段
- mini-code 必须先生成 `predictions.jsonl`
- Adapter 是必须的，因为官方不知道如何调用 mini-code
- 评测时只看 patch，不看回答文本
- 500 个任务会消耗大量时间、磁盘、Docker 资源和模型调用成本
- 建议先小规模跑通，再跑全量

---

## 十二、一句话总结

跑 SWE-bench Verified 测 mini-code 的流程是：先用 Adapter 批量调用 mini-code 解决 500 个 issue，并导出 `predictions.jsonl`；再用 SWE-bench 官方 harness 读取这些 patch，借助预构建镜像自动应用 patch、运行测试，最终统计 resolved rate。

---

## 十三、几个关键概念

### 1. `git diff` 是什么

`git diff` 是 Git 命令，用来查看当前代码相比原始版本具体改了什么。

例如 mini-code 修改代码后，执行：

```bash
git diff
```

会输出一段改动文本，说明哪些文件被改了、哪些行被删除、哪些行被新增。

简单理解：

```text
git diff = 查看 mini-code 最终改了什么
```

### 2. patch 是什么

patch 就是一份“代码改动包”。

它通常就是 `git diff` 的输出内容，可以被保存，也可以被应用到另一个相同版本的仓库上。

例如：

```bash
git diff > fix.patch
git apply fix.patch
```

简单理解：

```text
patch = mini-code 生成的代码修改内容
```

### 3. `predictions.jsonl` 是什么

`predictions.jsonl` 是交给 SWE-bench 官方评测器的结果文件。

它里面每一行对应一个 SWE-bench 任务，通常包含：

```json
{
  "instance_id": "django__django-12345",
  "model_name_or_path": "mini-code",
  "model_patch": "diff --git a/..."
}
```

其中最关键的是 `model_patch`，它保存的就是 mini-code 最终生成的 patch，也就是 `git diff` 的内容。

如果 SWE-bench Verified 有 500 个任务，那么 `predictions.jsonl` 通常就会有 500 行，每一行保存一个任务的 patch。

### 4. 三者关系

```text
mini-code 修改代码
    ↓
git diff 查看改动
    ↓
git diff 的内容就是 patch
    ↓
Adapter 把 patch 写入 predictions.jsonl
    ↓
SWE-bench 读取 predictions.jsonl 并评测
```

一句话理解：

```text
git diff = 看改了什么
patch = 这些改动本身
predictions.jsonl = 收集所有任务 patch 的评测输入文件
```
一套用来自动运行、控制、测试某个系统的框架或工具  harness

swe-bench是 ICLR2024年发布的，是一个用于评估代码修复模型的基准集。

官方 codex+gpt5.5  88.7%