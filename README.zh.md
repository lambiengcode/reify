<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.svg">
    <img src="assets/logo.svg" width="260" alt="Reify">
  </picture>
</p>

<p align="center">
  <em>业务逻辑只装在一个人的脑子里。<br>Reify 把它取出来，而不需要那个人去写文档。</em>
</p>

<p align="center">
  <sub>已经装好了？运行 <code>reify upgrade</code></sub>
</p>

<p align="center">
  <strong>确定性知识图谱 &middot; 每条结论皆有引用 &middot; 从 BA 文档到代码 &middot; 单一二进制、无常驻进程 &middot; 从不打开 socket</strong>
</p>

<p align="center">
  <a href="https://github.com/lambiengcode/reify/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/lambiengcode/reify/ci.yml?style=flat-square&label=ci" /></a>
  <a href="https://github.com/lambiengcode/reify/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/lambiengcode/reify?style=flat-square&color=blue" /></a>
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/github/license/lambiengcode/reify?style=flat-square&color=blue" /></a>
  <a href="#install"><img alt="平台" src="https://img.shields.io/badge/平台-macOS%20%7C%20Linux-blue?style=flat-square" /></a>
  <a href="#privacy"><img alt="网络调用: 0" src="https://img.shields.io/badge/网络调用-0-success?style=flat-square" /></a>
  <a href="#development"><img alt="369 测试" src="https://img.shields.io/badge/测试-369-success?style=flat-square" /></a>
</p>

<p align="center">
  <a href="#swebench"><img alt="SWE-bench retrieval 84.6%" src="https://img.shields.io/badge/SWE--bench%20检索-84.6%25-blueviolet?style=flat-square" /></a>
  <a href="#what-it-reads"><img alt="11 语言" src="https://img.shields.io/badge/语言-11-informational?style=flat-square" /></a>
  <a href="#what-it-reads"><img alt="10 文档格式" src="https://img.shields.io/badge/文档格式-10-informational?style=flat-square" /></a>
  <a href="#architecture"><img alt="Rust" src="https://img.shields.io/badge/rust-1.75%2B-dea584?style=flat-square&logo=rust&logoColor=white" /></a>
</p>

<p align="center">
  <a href="#claude-code"><img alt="Claude Code" src="https://img.shields.io/badge/Claude%20Code-supported-2da44e?style=flat-square" /></a>
  <a href="#other-agents"><img alt="Cursor" src="https://img.shields.io/badge/Cursor-supported-2da44e?style=flat-square" /></a>
  <a href="#other-agents"><img alt="Codex" src="https://img.shields.io/badge/Codex-supported-2da44e?style=flat-square" /></a>
  <a href="#other-agents"><img alt="OpenCode" src="https://img.shields.io/badge/OpenCode-supported-2da44e?style=flat-square" /></a>
  <a href="#other-agents"><img alt="Aider" src="https://img.shields.io/badge/Aider-supported-2da44e?style=flat-square" /></a>
  <a href="#mcp"><img alt="MCP" src="https://img.shields.io/badge/MCP-3%20tools-2da44e?style=flat-square" /></a>
</p>

<p align="center">
  <strong>在 SWE-bench Verified 上，Reify 有 84.6% 的概率把必须改动的文件送到模型面前 —— grep 只有 6.6% &middot; 500 个真实 issue，别人的基准 &middot; 从不打开任何 socket</strong><br>
  <sub>真实模型，142 个任务，全部取自 ERPNext、OFBiz、OpenMRS 和 Medusa 中真实合并的提交；每个索引都构建在这些改动<em>尚不存在</em>的提交上。那是<em>检索</em>：把正确的文件送到模型面前。而在端到端的补丁正确性上，目前一个 BM25 基线解决的 issue <em>比</em> Reify 更多，<a href="#swebench">说明这一点的那一节</a>与本节同样醒目。<a href="benchmarks/REPORT.md">完整报告</a> &middot; <a href="#reproducing-the-benchmark">自行复现</a>。</sub>
</p>

<p align="center">
  <img src="assets/demo.gif" width="920" alt="在真实 ERPNext 索引上的终端功能巡览：reify index 重建图谱；reify context 在 1500 token 预算内为「新增折扣档位」编译简报；reify why 针对 customer.py 的某一行返回它的调用方、它写入的表、历史上与它一同变更的文件，以及解释它的 2022-2025 年提交；reify impact 跨多跳追踪 check_credit_limit 的影响半径；reify explain 展示信用额度这一概念出现过的每个文件；reify context --toon 以 agent 格式输出同样的事实。">
</p>

<p align="center">
  <sub>demo 中的每一条命令都是真实执行的，跑在真实的 ERPNext 索引上。录制脚本已<a href="assets/demo.tg">提交入库</a>（由 <a href="https://github.com/aayushadhikari7/termgif">termgif</a> 录制）；如果这段动图哪天与工具本身对不上，就重新录制动图。</sub>
</p>

## <a id="two-minutes"></a>两分钟得到第一个答案

```bash
curl -fsSL https://raw.githubusercontent.com/lambiengcode/reify/main/install.sh | sh
cd your-repository
reify init --write-agent-instructions   # 通过 AGENTS.md / CLAUDE.md 接入你的 agent
reify index                             # 5000 个文件 4.6 秒；改动一个文件后 0.7 秒
reify context "你即将进行的改动" --toon
```

<sub>一个静态二进制 —— 无常驻进程、无配置、无 API key，且每个发布版都附带 SHA-256
校验和，<code>reify upgrade</code> 会在安装前先校验。改主意了？<code>reify uninstall</code>
删除二进制，<code>reify uninit</code> 清理单个仓库，二者都会先打印计划。各 agent 的接入、
钩子与 MCP：<a href="#install">安装</a>。</sub>

<p align="center">
  <a href="README.md">English</a> &middot; <a href="README.vi.md">Tiếng Việt</a> &middot; <strong>简体中文</strong>
</p>

---

**目录**

- [两分钟得到第一个答案](#two-minutes)
- [只有一个人懂的问题](#the-one-person-problem)
- [它到底给你什么](#what-it-actually-gives-you)
- [改造前 / 改造后](#before--after)
- [SWE-bench Verified](#swebench) — 84.6% 对 grep 的 6.6%
- [数据](#numbers) — [纯检索表现](#retrieval-alone) · [记分卡](#the-scorecard) · [它失效的地方](#where-it-doesnt-work)
- [工作原理](#how-it-works) — [通往代码的四座桥](#four-bridges)
- [它能读什么](#what-it-reads)
- [多语言](#multilingual)
- [安装](#install) — [Claude Code](#claude-code) · [其他 agent](#other-agents) · [MCP](#mcp) · [接入模型](#optional-a-model)
- [命令](#commands)
- [隐私](#privacy)
- [架构](#architecture) — [实测性能](#measured-performance)
- [复现基准测试](#reproducing-the-benchmark)
- [开发](#development)
- [常见问题](#faq)
- [路线图](#roadmap) · [项目状态](#status) · [许可证](#license)

## <a id="the-one-person-problem"></a>只有一个人懂的问题

你的系统已经十一年了。业务逻辑体量巨大，而且几乎没有文档 —— SharePoint 上还留着
2019 年的几份 BA 文档，其中一部分至今仍然成立。

**只有一个人真正懂它。** 他休假也得带着手机。你没法靠招人来分担，因为一个新来的开发
者要花上大半年才能真正上手，而他需要吸收的那些知识哪儿都没写 —— 它在那一个人的脑子
里，而那个人忙得根本没空写下来。

于是你把 AI 编码 agent 对准了这套系统。agent 在新代码上表现惊艳，在这里却毫无用处。
它读错了四十个文件，漏掉了真正关键的那条规则，然后信心十足地改掉了某个客户正依赖的
行为。最后还得由那唯一懂的人来 review —— 而这正是你本想消除的瓶颈。

**Reify 把这些知识从一个人的脑子里取出来，变成你的 agent 和新同事都能用的形式 ——
而且不需要任何人去写那份他们永远不会写的文档。** 它编译的是已经存在的东西：代码、
没人读的 BA 文档、数据库 schema，以及十一年来解释「为什么」的提交信息。

### 这听起来像你的代码库吗？

- [x] 比团队里最新的成员年纪还大
- [x] 业务规则散落在代码、存储过程、配置以及某个人的记忆里
- [x] 没有开发文档。只有几份 Word 或 PDF 的 BA 文档，新旧不明
- [x] 「去问老王，那块是他写的」是技术问题的常规答案
- [x] 新人上手时间以月计
- [x] *现有*的文档和代码互相矛盾，而没人知道矛盾在哪
- [x] AI agent 在你的业余项目上跑得很好，在这套系统上一败涂地
- [x] 源代码不允许离开公司

Reify 就是为这种情况而生的。如果以上都不像你，你大概不需要它 —— 见[常见问题](#faq)。

## <a id="what-it-actually-gives-you"></a>它到底给你什么

三个问题，答案来自证据，而不是模型的记忆：

| 问题 | 命令 | 谁在问 |
|---|---|---|
| *这段代码为什么存在？* | `reify why <file>:<line>` | 入职第二天的新人 |
| *我改了它，什么会坏？* | `reify impact "<symbol>"` | 动手改的那个人 |
| *动手之前我必须知道什么？* | `reify context "<task>"` | **你的 AI agent，每一次** |

第三个才是关键。它交给 agent 一份最小集合：需要的规则、出处引用、代码片段和已知矛盾
—— 再无其他。

### 给那个所有人都依赖的人

你不必写文档。Reify 读取已经存在的内容；在它只能靠猜的地方，`reify concepts --suggest`
会给你一份术语表草稿，你花一个下午修订即可，而不是从零撰写。你花十分钟做的订正，比
别人花一周考古更有价值。

### 给刚加入的人

```bash
reify report                       # 我到底在看什么
reify explain "信用额度"            # 在它出现过的每一种语言里
reify flow "订单审批"               # 代码路径，按顺序
reify conflicts                    # 哪些文档在骗我
```

## <a id="before--after"></a>改造前 / 改造后

你让 agent 修改订单审批阈值。它 grep 了 `50000000`，找到一处，改掉，提交。它永远不会
知道 BRD 里写着企业客户必须走审批，而代码从 2019 年起就一直悄悄绕过了这一步。

用 reify：

```
$ reify why erpnext/selling/doctype/sales_order/sales_order.py:812

  [CONFLICT] documentation and implementation disagree about approval
    documented   Corporate customers must require approval    docs/BRD-42.md:6
    observed     Corporate customers bypass approval          sales_order.py:812

  Called by     3 services, 1 batch job
  Writes        tabSales Order, approval_log
  History       8a31c2f  2019-04-17  fix: enterprise approval flow
```

其中四段里有三段，是 grep 在结构上根本产不出来的。

## <a id="swebench"></a>数据：在一个不是我们设计的基准上

下面那份四代码库基准是我们自己的 —— 这正是必须再跑一份别人的基准的理由。
**[SWE-bench Verified](https://openai.com/index/introducing-swe-bench-verified/)**
包含来自十二个知名 Python 项目的 500 个真实 GitHub issue，每个都钉在该 issue 被提出时
的 `base_commit` 上 —— 与 Reify 自有基准相同的「先建索引、后有改动」协议，只不过是别人
写的。任务是一份普通的问题报告；正确答案是被采纳的修复实际改动的那些文件。

| SWE-bench Verified 上的检索，n=500 | 提供了修复所改动的某个文件 | MRR | 提供了**全部**此类文件 | token 中位数 |
|---|--:|--:|--:|--:|
| grep, content | 6.6% <sub>[4.7–9.1]</sub> | 0.06 | 5.6% | 3,998 |
| grep, paths | 9.0% <sub>[6.8–11.8]</sub> | 0.06 | 7.8% | 3,996 |
| **reify**, 单轮 | **66.0%** <sub>[61.7–70.0]</sub> | 0.43 | 59.0% | **3,466** |
| **reify**, 三轮 | **84.6%** <sub>[81.2–87.5]</sub> | 0.45 | 77.0% | 9,174 |

**单轮 Reify 在 310 个实例上胜过 grep，仅在 13 个上落败 —— 而且花的 token 更少**
（3,466 对 3,998）。三轮则是 395 比 5（精确 McNemar 检验 p ≈ 7 × 10⁻¹¹⁰）。这不是一次
势均力敌的测量，而且它是本文档中最干净的数字，正因为任务、代码库和标准答案全都来自
别处。

按代码库，三轮对内容 grep：

| | grep | reify ×3 | | | grep | reify ×3 |
|---|--:|--:|---|---|--:|--:|
| django (n=231) | 6% | **88%** | | astropy (n=22) | 0% | **77%** |
| sympy (n=75) | 7% | **77%** | | xarray (n=22) | 9% | **91%** |
| sphinx (n=44) | 7% | **75%** | | pytest (n=19) | 26% | **84%** |
| matplotlib (n=34) | 0% | **91%** | | pylint (n=10) | 10% | **60%** |
| scikit-learn (n=32) | 9% | **88%** | | requests (n=8) | 0% | **100%** |

**它证明了什么，没证明什么。** 它测的是*检索* —— 必须改动的文件是否被送到模型面前 ——
而不是模型随后能否写出正确的补丁。Verified 只有 Python，因此它对[下文](#where-it-doesnt-work)
的现代 TypeScript 弱项只字未提。而且这些仓库名气大到模型已部分背诵；这影响的是模型的
*答案*，而非检索器提供哪些文件，并且这里每一组都跑在同一提交的同一份索引上。可用
[`benchmarks/swe/`](benchmarks/swe/) 中的驱动脚本复现。


### 端到端的结果，而且它对我们不利

检索并不是这个项目最终的主张 —— 解决 issue 才是。因此同一份基准被放进 SWE-bench 论文
自己的「检索增强生成补丁」协议：一个模型、一份上下文预算，各组之间唯一的差别是检索器，
每个补丁都由 Docker 中的 **SWE-bench 官方评测器**判定。101 个分层抽样实例，其中 72 个
在两组下都被判定过。

| 解决了 issue | | 95% 置信区间 |
|---|--:|---|
| BM25 | **18.1%**（13/72） | [10.9–28.5] |
| Reify | 11.1%（8/72） | [5.7–20.4] |

BM25 解决了 8 个 Reify 没解决的实例；Reify 解决了 3 个 BM25 没解决的（精确 McNemar
检验 p = 0.23）。在这个样本量下这不是显著差异 —— 但点估计偏向 BM25，而这里就按这个方向
报告，因为结果本来就是这个方向。

**诊断比数字更有价值。** 在 BM25 解决而 Reify 没解决的那 8 个实例中，有 5 个
**Reify 其实已经提供了修复所改动的文件**。所以这些并不是检索失败。把正确的文件送到模型
面前，和给模型足够写出补丁的材料，是两个不同的问题，而实测显示 Reify 在前者上远强于
后者。

一条限制，作为限制而非辩解陈述：该协议把 Reify 当作*文件排序器*使用，喂入的是整份文件，
这恰恰丢掉了 `reify context` 真正产出的东西 —— 代码片段、规则、出处引用、冲突，以及按
预算编排的阅读计划。改为喂入编译好的上下文，是显而易见的下一个实验。它还没有被跑过，
所以它目前什么也没证明。

## <a id="numbers"></a>数据：在四个刻意挑难的代码库上


诚实的度量方式，是让真实模型去做真实任务：工单取自已合并的提交，提示词就是开发者本人
对这次改动的描述，正确答案就是他们实际改动的文件。**每个索引都构建在这些改动尚不存在
的提交上**，所以被问到的代码是真的还没出现。四个代码库，选取时有意挑难啃的；有几组
对照条件的设计目的是推翻结论，而不是支持它。

<p align="center">
  <img src="assets/benchmark-agent.svg" width="860" alt="四个代码库上各条件的命中率，误差线为 95% 置信区间。ERPNext，40 个任务：无上下文 22%，三倍预算的 grep 50%，reify 三轮 75%，完美上下文 100%。OFBiz，40 个任务：0%、28%、78%、100%。OpenMRS，22 个任务：0%、32%、59%、100%。Medusa，40 个任务：0%、24%、26%、100% —— 在 Medusa 上 reify 与 grep 重叠。">
</p>

主对比是成本对齐的：Reify 迭代三轮（agent 读了、没找到、把已读文件排除后再问一次），
所以对照组直接把同样的三倍预算一次性交给 grep。

| 模型参与，命中率 | grep ×3 预算 | **reify ×3 轮** | 差距 | 95% 置信区间重叠？ |
|---|--:|--:|--:|---|
| ERPNext（Python/JS），n=40 | 50% | **75%** | +25 | 勉强重叠 |
| OFBiz（Java + XML），n=40 | 28% | **78%** | +50 | 不重叠 |
| OpenMRS（Java），n=22 | 32% | **59%** | +27 | 勉强重叠 |
| Medusa（现代 TS），n=40 | 24% | **26%** | +2 | **完全重叠 —— 没有优势** |

> **关于第四行的说明：** Medusa 的 +2 是平局，不是胜利，而且它以完整篇幅留在主表中，
> 而不是被藏进脚注。Reify 大胜的仓库与它赢不了的仓库之间的分界线是测出来的，不是猜的
> —— 见[它失效的地方](#where-it-doesnt-work)。

**每个代码库上的对照组：** 完美上下文处处拿到 100%，说明检索质量就是全部胜负手。形状
完全相同的诱饵上下文只有 0–12%，说明起作用的是内容而非格式。完全不给代码库访问权时，
模型在三个代码库上是 0%，在 **ERPNext 上是 22%** —— 它部分背下了这个最有名的仓库，
而这正是另外三个代码库存在的理由，也是每一个「差距回收率」都要减去这个基线的理由。
约 1000 次模型调用中有 7 次失败，全部发生在 Medusa 上；失败调用被排除在比率之外，绝不
计为未命中。

单轮成绩，如实记录：reify 55/68/41/15，grep 30/12/41/21，预算相同 —— 在 OFBiz 上，
*一轮* reify 就已经领先 grep 56 个百分点。

### <a id="retrieval-alone"></a>纯检索表现，不涉及模型

<p align="center">
  <img src="assets/benchmark-retrieval.svg" width="860" alt="各代码库中，被改动文件出现在候选里的任务占比。ERPNext：grep 10%，路径 grep 18%，reify 57%，reify 三轮 75%。OFBiz：12%、15%、70%、78%。OpenMRS：32%、18%、41%、55%。Medusa：18%、18%、18%、28%。">
</p>

| 被改动的文件是否被提供 | grep | reify（MRR） | **reify ×3** |
|---|--:|--:|--:|
| ERPNext | 10% | 57%（0.45） | **75%** |
| OFBiz | 12% | 70%（0.45） | **78%** |
| OpenMRS | 32% | 41%（0.27） | **55%** |
| Medusa | 18% | 18%（0.09） | **28%** |

### <a id="the-scorecard"></a>记分卡，对照动工前设定的目标

七项目标在改进工作开始之前就已登记。**七项中达成一项**（实测四个代码库）。命中率、
差距回收率、跨代码库落差、MRR、精确率和端到端完成率都没达到各自的门槛。进步是真的
—— 目标本就有意定得很高，而用诚实数字换来的未达标，好过用宽松门槛换来的达标。

### <a id="where-it-doesnt-work"></a>它失效的地方

**Medusa** —— 一个现代的、拆分良好的 TypeScript monorepo —— 是尚未解决的问题，而且它
推翻了这个项目的立项假设。老旧的 Java 系统本以为是难啃的，结果它们是*最好啃*的。
Medusa 的任务描述的是界面行为（「去掉重复的云登录按钮」），其用词几乎与代码不相交，
提交历史又是 squash 过的 PR 合并，Reify 目前读取的任何东西都无法弥合这道鸿沟。迭代把
检索从 18% 提到 28%；接入模型后是 26% 对 grep 的 24%；置信区间完全重叠。

早先的假设 ——「Reify 的优势随已声明词汇量增长」—— 也没能通过四个代码库的检验。OFBiz
并不像 ERPNext 那样声明大量词汇，却拿到了所有代码库中最大的领先幅度。四个代码库真正
的分界线是：*提交历史和文件命名，是否说着和任务描述同一套词汇*。说着同一套词汇时，
Reify 能回收 54–62% 的理论差距；不说时（Medusa），它就只是结构更好的 grep。

## <a id="how-it-works"></a>工作原理

**确定性优先，语义其次，LLM 最后。** 在这个构建中，除非你自行配置，否则完全不存在 LLM，
而且所有命令在没有 LLM 时照样可用。

```
1. 在 AST 里吗？        → 符号、调用、导入、继承
2. 在数据层里吗？        → 表、列、ORM 映射、内嵌 SQL
3. 在文档里吗？          → 章节，按标题引用
4. 在 git 里吗？         → 谁引入的、什么修复了它、什么和它一起变动
5. 有在哪里声明过吗？     → 术语表、实体元数据、翻译文件
6. 只有到这一步才推断     → 并标记为 INFERRED，附上证据
```

每一条结论都带着它的来源，以及可以信任到什么程度：

| | |
|---|---|
| `CONFIRMED` | 直接从源文件中读出 |
| `OBSERVED` | 由已确认事实确定性地推导得出 |
| `INFERRED` | 一条启发式规则。**据此行动前请先核对出处** |
| `CONFLICTED` | 两个来源互相矛盾。改动行为之前先解决它 |
| `UNKNOWN` | 明确标记为未解决，因此「没有信息」永远不会被当作证据 |

`Status::Unknown` 被刻意设为 `Default`。任何忘记声明自身依据的东西，都会落到那个 agent
不得据以行动的状态上。

### <a id="four-bridges"></a>从业务词汇通往代码的四座桥

按精确度从高到低排列。最后一座桥，是 Reify 在一个什么都没声明的代码库上仍然能用的原因。

| 桥 | 来源 | 何时可用 |
|---|---|---|
| **显式声明** | `.reify/glossary.toml`、实体元数据、ORM 映射 | 有人或某个框架把它写下来了 |
| **翻译** | i18n 表、message bundle | 产品做过本地化 |
| **共现** | 同时点到代码名字的文档标题 | 有文档 |
| **代码词汇** | 标识符反复出现的词组 | **永远可用** |

最后一座桥只在前三座未覆盖的范围内运行，因此它是填补空白，而不是与更强的证据竞争。
样板词的过滤方式，是测量哪些词在*这个代码库内部*无处不在，而不是依赖一份
`get`/`set`/`manager` 的现成清单 —— 那种清单只适配一种技术栈，换一种就失灵。

## <a id="what-it-reads"></a>它能读什么

**代码，11 种语言。** Python、TypeScript、JavaScript、Java、Go、C#、Rust、Ruby、PHP、
C/C++、Kotlin，外加 SQL。每一种都有一个测试断言它能产出容器、可调用体*以及*调用关系
—— 因为少映射一个语法节点，你会得到一个看起来很健康、实则每个文件只有一个符号的索引。
这不是假设：它真的在 Java 上发生过，现在测试能抓住它。

**文档，不管分析师用什么写的。** 这是大多数代码工具跳过的部分，而对许多这类系统来说，
它是仅有的文档。

| | |
|---|---|
| 原生支持 | Markdown、纯文本、HTML（含 Confluence 导出） |
| Zip + XML | DOCX、ODT、XLSX、PPTX |
| 委托外部 | PDF、老式二进制 DOC、RTF |

没有可用纯 Rust 读取器的格式会交给外部转换器（`pdftotext`、`mutool`、`antiword`、
`textutil`、`soffice`），按顺序依次尝试。一个都没装时，Reify 会**列出它尝试过的每一个
工具以及安装方法**，而不是悄无声息地什么都不索引。

**团队声明过的任何东西。** Frappe DocType JSON、Hibernate ORM 映射、Java 与 Spring 的
`.properties` message bundle、i18n CSV 表。这是一个代码库能提供的最高精度词汇来源，
因为应用程序自己就在读它，所以它始终是对的。

## <a id="multilingual"></a>多语言

没有哪种语言是基准语言，英语也不是。概念 id 是不透明的，每个标签都带语言标记，因此一条
越南语、泰语、韩语或德语的需求，是通过概念层而不是 embedding 模型抵达英文代码的 ——
这也是为什么答案带回来的是行号，而不是一个相似度分数。

在翻译文件和 message bundle 上可识别约 60 种 locale。义务性与豁免性表述能在 11 种语言中
被识别，因此用其中任何一种写下的规则都会被作为规则挖掘出来。

有三件事只有在离开拉丁字母之后才会暴露，而它们全都先在这里暴露过：

- **泰语、老挝语、高棉语、日语和中文不用空格分词**，于是按词建立的索引会存下一个巨大的
  token，而搜索其中的某个词却什么都匹配不到。针对非 ASCII 内容有一套 trigram 子串索引；
  纯 ASCII 的代码库永远不必为它付出代价。
- **韩语把助词黏在词干后面。** `승인` 会变成 `승인을`，整词匹配两个都找不到。
- **句子长度不能靠数空格来算**，否则每一条泰语需求都会因为「太短，不像规则」而被丢弃。

## <a id="install"></a>安装

```bash
curl -fsSL https://raw.githubusercontent.com/lambiengcode/reify/main/install.sh | sh
```

提供 macOS（Apple Silicon 与 Intel）和 Linux（x86_64 与 aarch64）的预编译二进制。
也可以从源码构建：

```bash
cargo install --path crates/reify-cli
```

然后，在任意代码库中：

```bash
reify init      # 告诉你它会索引什么、不会索引什么，以及为什么
reify index     # 5000 个文件 4.6 秒；改动一个文件后 0.7 秒
```

**升级干净，卸载彻底。** `reify upgrade` 用最新发布版替换当前二进制 —— 通过看得见的
`curl` 与 `tar` 子进程完成，从不内嵌 HTTP 客户端，并且在安装任何东西之前先校验
checksum；`--check` 只查询不安装，`REIFY_OFFLINE=1` 则直接拒绝整个命令。
`reify uninstall --yes` 只删除二进制本身；`reify uninit --yes` 删除单个仓库的
`.reify/` 存储和 `init` 写入的指令块。不带 `--yes` 时，两者都只打印计划。

<details>
<summary><strong>Shell 补全</strong></summary>

```bash
reify completions zsh  > ~/.zfunc/_reify
reify completions bash > /etc/bash_completion.d/reify
reify completions fish > ~/.config/fish/completions/reify.fish
```

</details>

### <a id="claude-code"></a>Claude Code

第 0 级 —— 基准测试测的就是这一级，也是建议起步的一级：

```bash
reify init --write-agent-instructions
```

它会往你的 `AGENTS.md` 或 `CLAUDE.md` 追加六行内容。没有协议，没有服务端，也不必为
每一轮对话缴纳 schema 税。

<details>
<summary><strong>编辑前置钩子，以及保持索引新鲜</strong></summary>

在每次编辑前注入一段风险提示：

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Edit|Write",
      "hooks": [{ "type": "command", "command": "reify preflight \"$CLAUDE_FILE_PATH\"" }]
    }]
  }
}
```

```
PREFLIGHT  erpnext/selling/doctype/sales_order/sales_order.py
  rules 7 · concepts 4 · tables 3 · dependants 22 · conflicts 1
  RISK: HIGH — documentation and implementation disagree about this file
```

不到 300 个 token，并有测试断言，因为它在每次编辑时都会运行。默认不阻断：会阻断编辑的
钩子会被卸载，然后它的警告也一并丢失。

保持索引最新：

```bash
printf '#!/bin/sh\nreify index >/dev/null 2>&1 &\n' > .git/hooks/post-merge
chmod +x .git/hooks/post-merge
cp .git/hooks/post-merge .git/hooks/post-checkout
```

</details>

### <a id="other-agents"></a>Codex、Cursor、OpenCode、Aider、Pi、Windsurf、Cline

不需要适配器 —— Reify 就是一个 CLI。把下面这段放进该工具会读取的指令文件里
（`AGENTS.md`、`.cursorrules`、`CONVENTIONS.md`、`.windsurfrules`、`.clinerules/`）：

```markdown
Before changing code here, run `reify context "<what you are about to do>" --toon`.
Run `reify why <file>:<line>` before modifying unfamiliar logic.
Run `reify impact "<symbol>"` before changing anything shared.
Treat INFERRED claims as leads to verify, not facts.
```

### <a id="mcp"></a>MCP

```bash
reify serve --mcp
```

三个工具 —— `reify_context`、`reify_why`、`reify_impact` —— 三个就是全部接口。MCP 服务端
的 schema 会在每个会话的每一轮被重新发送，所以一个为了节省上下文而生的工具，不该为了
送货再收一笔租金。有测试断言这些 schema 的开销低于 600 个 token。

### <a id="optional-a-model"></a>可选：接入一个模型

没有默认的模型提供方，在你明确开口之前什么都不会启用。

```toml
# .reify/llm.toml
command = ["ollama", "run", "llama3"]
```

Reify 会把提示词写入该命令的 stdin，或替换 `{prompt}` 参数。为什么是一条命令而不是一个
HTTP 客户端，见[隐私](#privacy)。

## <a id="commands"></a>命令

| 命令 | 作用 |
|---|---|
| `reify context "<task>"` | 一次改动所需的最小知识集，外加阅读计划。**最重要的那个。** `--toon` 输出面向 agent 的格式 |
| `reify why <file>:<line>` | 这是什么、谁调用它、它碰了哪些数据、什么改动过它 |
| `reify impact "<symbol>"` | 什么依赖它 —— 包括经由数据库产生的依赖，那里根本不存在调用边 |
| `reify explain "<term>"` | 一个业务概念，横跨它出现过的每一种语言、每一张表、每一个文件 |
| `reify flow "<process>"` | 承载某个业务流程的调用序列 |
| `reify conflicts` | 与代码相矛盾的文档 |
| `reify rules` | 挖掘出的业务规则，附证据 |
| `reify concepts --suggest` | 把挖掘结果变成术语表条目，交给你精简 |
| `reify preflight <file>` | 供编辑器钩子使用的风险提示行 |
| `reify report` | 系统记分卡 |
| `reify status` | 新鲜度、覆盖率，以及哪些内容被跳过了 |
| `reify llm status \| preview` | 是否配置了模型，以及究竟会发送什么内容 |
| `reify upgrade [--check]` | 用最新发布版替换当前二进制。唯一联网的命令；`REIFY_OFFLINE=1` 时被拒绝 |
| `reify uninstall --yes` \| `uninit --yes` | 删除二进制 \| 单个仓库的存储与指令块 |
| `reify serve --mcp` | 基于 stdio 的 Model Context Protocol |
| `reify completions <shell>` | 补全脚本 |

所有命令都支持 `--json`（对应带版本的 schema）和 `--budget <tokens>`。完整输出结构见
[docs/json-schema/](docs/json-schema/)。

**agent 应当索要 `--toon`。** JSON 在每条记录上重复一遍字段名；TOON 只声明一次每个区段
的列，然后每条记录一行 —— 实测**相同信息少用 57% 的 token**，而 `status` 仍是每行的第一
列。头部携带的是所输出字节本身的实测 token 开销，因此预算声明与实际负载不可能对不上。
MCP 的 `reify_context` 已经用 TOON 应答。

## <a id="privacy"></a>隐私

**你的源代码和业务文档永远不会离开这台机器。** Reify 不打开任何网络连接 —— 不是「默认
不打开」，而是根本没有。依赖树中不存在 HTTP 客户端，一旦出现一个，`cargo test` 就会让
构建失败。

对一家绝不允许专有代码接近云服务的公司来说，这是「可以评估的工具」和「无法评估的工具」
之间的区别。

| | |
|---|---|
| `Cargo.lock` 中的网络库 | 断言为零，在 CI 中执行 |
| 源码中的 socket | 断言为零，在 CI 中执行 |
| 子进程 | `git`、经过审查的文档转换器，以及 —— 仅限 `reify upgrade` —— `curl` 与 `tar`；每一个都在测试中被点名 |
| 执行你仓库里的代码 | 从不。tree-sitter 只做解析，不运行代码 |
| 存储位置 | `.reify/`，由 `reify init` 加入 gitignore |

模型辅助是一条**由你**配置的命令，而不是内嵌的客户端。本地模型无需额外代码即可工作，
没有任何凭据经过 Reify，`reify llm preview` 会在任何字节被发送之前先打印出确切内容，
而 `REIFY_OFFLINE=1` 会让它彻底不可达，无论配置文件里写了什么。

完整威胁模型，包括**未**覆盖的部分：[docs/privacy.md](docs/privacy.md)。

## <a id="architecture"></a>架构

每个代码库一个 SQLite 文件。没有图数据库，没有向量库，没有常驻进程。

```
  LAYER 4  Synthesis    optional model, cached, always INFERRED        llm.rs
  LAYER 3  Selection    seed → spread → budget knapsack → render       context.rs
  LAYER 2  Semantics    concepts, rules, conflicts       concepts.rs · rules.rs
  LAYER 1  Structure    symbols, calls, tables, sections, commits  extract/ · gitlog.rs
  LAYER 0  Substrate    walk, classify, hash, store      discover.rs · store.rs
```

**增量索引与完整重建逐字节一致**，由一个属性测试断言：它施加随机的编辑序列，然后比较
规范化导出结果。每个阶段拥有互不相交的边类型集合和各自的失效触发条件，这正是上述结论
成立的原因。细节见 [docs/architecture.md](docs/architecture.md)。

### <a id="measured-performance"></a>实测性能

ERPNext，5064 个文件，8 核 M 系列笔记本。

| | 实测 |
|---|--:|
| 完整索引，不用模型 | 4.6 秒 |
| 重新索引，无任何改动 | 0.6 秒 |
| 重新索引，改动一个文件 | 0.7 秒 |
| `reify context` | 57 毫秒 |
| `reify impact` | 0.2 毫秒 |
| `reify why` | 205 毫秒 —— 因为要起一个 `git log -L` 子进程；不带它约 5 毫秒 |
| 峰值内存，完整索引 | 224 MB |
| 存储体积 | 47 MB（144 MB 工作区的 33%） |

完整索引一度需要 **78 秒**，直到全文索引改用节点 id 作键。`uid` 在 FTS5 中是
`UNINDEXED`，于是 `DELETE ... WHERE uid = ?` 每处理一个节点就要全表扫描一次 —— 平方级
复杂度，而且在按阶段计时之前完全看不见。改动一个文件一度需要 **5.9 秒**，直到那些覆盖
全库的阶段学会了在输入可证明未变时直接跳过。

`REIFY_TIMING=1 reify index` 会打印出发现这两个问题的分阶段耗时明细。

## <a id="reproducing-the-benchmark"></a>复现基准测试

上面表格里没有一个数字是手打的。任务集、每个任务的原始结果和图表全部已提交入库。

```bash
# 1. 从真实的已合并提交中冻结一个任务集，截止于选定的基准提交之前
reify-bench tasks --repo <repo> --after <base-sha> --out benchmarks/tasks/mine.json

# 2. 在该基准提交上建索引，使被问到的改动确实还不存在
git worktree add /tmp/base <base-sha>
reify -C /tmp/base init && reify -C /tmp/base index

# 3. 先纯检索，再接入模型
reify-bench run   --repo /tmp/base --tasks benchmarks/tasks/mine.json --out results/
REIFY_LLM_COMMAND='<your model cli> {prompt}' \
reify-bench agent --repo /tmp/base --tasks benchmarks/tasks/mine.json --out results/

# 4. 报告与图表，由原始结果生成
reify-bench report --in results/ --out benchmarks/REPORT.md
reify-bench chart  --results "Mine=results/" --out assets/
```

任务集在任何一组条件运行之前就已冻结。报告中包含一节 **"Where Reify lost"**，逐条列出
基线胜出的每一个任务，而且它是文档的必需部分，不是可选项。

## <a id="development"></a>开发

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo bench -p reify
```

全部在 CI 中强制执行，另外还有一次在禁止出站网络环境下的完整测试。`cargo test` 包含
`crates/reify/tests/offline.rs`，一旦有网络库进入依赖树就会让构建失败。

Fixture 位于 [`fixtures/minierp`](fixtures/) —— 一个小型业务系统，其中的知识是*预先埋好*
的：一条有文档的规则、与之矛盾的代码、一个魔法数字、一个双语概念，以及仅通过共享表存在
的跨模块耦合。Reify 对它做出的每一条结论都有已知的正确答案，所以那里一旦出错就无可辩驳。

新增一种语言 = 一个语法、一份节点类型映射、`classify` 里的一个分支，加一个 golden 测试。
哪些设计规则是承重的、哪些只是风格问题，见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## <a id="faq"></a>常见问题

**我们完全没有开发文档。只有 BA 文档，而且很旧了。**
这正是 Reify 为之而生的场景。它能读 DOCX、PDF、XLSX 等格式，把它们切成可引用的章节，
并且 —— 关键在于 —— 告诉你它们在哪里与代码*相矛盾*，于是一份旧文档就从陷阱变成了证据。
一份文档都没有时，它会退回到代码自身的词汇，`why`、`impact` 和历史依然可用。

**我们唯一的专家没时间帮忙搭这套东西。**
不需要他。`reify init && reify index` 完全不需要他参与。如果能借到他一个下午，
`reify concepts --suggest` 会把 Reify 挖掘出的内容变成术语表草稿，他只需修订而非撰写
—— 而[数据](#numbers)一节表明，已声明的词汇正是收益的来源。

**这真能让我们招得到人吗？**
它消除的是一个具体的瓶颈：新来的开发者或 agent，无法在不打扰别人的情况下自己搞清楚代码
*为什么*是现在这样。这是上手过程中真实存在的一部分，但不是全部。任何声称一个工具能替代
十一年上下文的人，都是在卖东西。

**我必须写术语表吗？**
不必，没有术语表 Reify 照样能用。显式声明的术语表仍然是你能提供的最高精度词汇 ——
`reify concepts --suggest` 会写好初稿供你精简 —— 但四个代码库的基准测试显示，更强的预测
因素是：你的提交历史是否说着和工单同一套词汇。如果你的团队认真写提交信息，Reify 已经在
读十一年份的带标注样本了。

**这又是一个 RAG 吗？**
没有向量数据库，没有 embedding 模型，也没有分块。检索是词法加图结构的，这正是每个答案
都带着行号而不是相似度分数的原因。

**我的仓库只有 3000 行。该用吗？**
不该。用 ripgrep。大约 2 万行代码以下，Reify 给不了你 grep 加滚轮给不了的东西。

**它会把我的专有代码发到什么地方吗？**
不可能。二进制里没有 HTTP 客户端，一旦出现一个，测试就会让构建失败。如果你配置了模型
提供方，那是你自己选的一条命令，而且 `reify llm preview` 会先给你看确切的字节。

**为什么 `reify why` 比别的命令慢？**
它要外调 `git log -L` 来获得精确的行级历史。带它 205 毫秒，不带约 5 毫秒。仍在待办清单上。

**conflicts 在我的仓库里什么都没找到。是坏了吗？**
多半没坏。触发检测需要五个条件同时成立，并且被强烈地偏向沉默 —— 因为一个动辄狼来了的
冲突检测器，第二周就会被关掉，连同它那些真阳性一起。它在 ERPNext 上找到 0 条（那个仓库
几乎没有规格说明文字），在 fixture 上正好找到 1 条，那里预埋了一条。

**"reify" 是什么意思？**
把抽象之物变得具体。这些知识一直都在，只是从来没成为一个文件。

## <a id="roadmap"></a>路线图

第一轮改进已经完成。历史先验（每个已合并的提交都是一个带标注的样本：提交信息 ≈ 工单，
改动文件 = 答案）、测试到代码的边、迭代式精修，以及第四个代码库都已落地；权重拟合没有
通过留出验证，按其事前登记被回退；记分卡停在七项目标中达成一项，每个数字都印在它的门槛
旁边。尚未解决的问题是现代 TypeScript 的情形：人们描述界面改动的说法，和代码里的写法
之间，那道词汇鸿沟目前还没有任何东西能填上。

## <a id="status"></a>项目状态

尚早，但有实测。已知的未达标项，全部写明而非掩埋：存储占工作区的 33%，目标是 5%；
`reify why` 耗时 205 毫秒，目标是 20 毫秒；Windows 尚未测试。

## <a id="license"></a>许可证

[Apache-2.0](LICENSE)。含专利授权，所以 agent 厂商真的可以把它发出去。
