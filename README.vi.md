<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.png">
    <img src="assets/logo.png" width="300" alt="Reify">
  </picture>
</p>

<p align="center">
  <em>Logic nghiệp vụ nằm trong đầu một người.<br>Reify lấy nó ra, mà không bắt ai phải ngồi viết tài liệu.</em>
</p>

<p align="center">
  <sub>Đã cài rồi? Chạy <code>reify upgrade</code></sub>
</p>

<p align="center">
  <a href="https://github.com/lambiengcode/reify/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/lambiengcode/reify/ci.yml?style=flat-square&label=ci" /></a>
  <a href="https://github.com/lambiengcode/reify/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/lambiengcode/reify?style=flat-square&color=blue" /></a>
  <a href="https://lambiengcode.github.io/reify/"><img alt="Documentation" src="https://img.shields.io/badge/docs-lambiengcode.github.io-2da44e?style=flat-square" /></a>
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/github/license/lambiengcode/reify?style=flat-square&color=blue" /></a>
  <a href="#swebench"><img alt="SWE-bench retrieval 84.6%" src="https://img.shields.io/badge/SWE--bench%20retrieval-84.6%25-blueviolet?style=flat-square" /></a>
  <a href="#privacy"><img alt="network calls: 0" src="https://img.shields.io/badge/network%20calls-0-success?style=flat-square" /></a>
</p>

<p align="center">
  <a href="#other-agents"><img alt="Claude Code" src="https://img.shields.io/badge/Claude%20Code-supported-2da44e?style=flat-square" /></a>
  <a href="#other-agents"><img alt="Cursor" src="https://img.shields.io/badge/Cursor-supported-2da44e?style=flat-square" /></a>
  <a href="#other-agents"><img alt="Codex" src="https://img.shields.io/badge/Codex-supported-2da44e?style=flat-square" /></a>
  <a href="#other-agents"><img alt="OpenCode" src="https://img.shields.io/badge/OpenCode-supported-2da44e?style=flat-square" /></a>
  <a href="#mcp"><img alt="MCP" src="https://img.shields.io/badge/MCP-3%20tools-2da44e?style=flat-square" /></a>
</p>

<p align="center">
  <strong>Trên SWE-bench Verified, Reify đặt đúng file cần sửa trước mặt mô hình 84,6% số lần — grep chỉ được 6,6% &middot; 500 issue thật, benchmark của người khác &middot; không bao giờ mở socket</strong><br>
  <sub>Một mô hình thật, 142 task lấy từ commit đã merge thật ở ERPNext, OFBiz, OpenMRS và Medusa; mỗi index được dựng tại một commit <em>trước khi</em> những thay đổi đó tồn tại. Đó là <em>truy xuất</em>: đúng file, đặt trước mặt mô hình. Còn về tính đúng đắn của bản vá đầu-cuối, hiện một baseline BM25 giải quyết được <em>nhiều</em> issue hơn Reify, và <a href="#swebench">mục nói điều đó</a> cũng nổi bật đúng như mục này. <a href="benchmarks/REPORT.md">Bản viết đầy đủ</a> &middot; <a href="#reproducing-the-benchmark">tự chạy lại</a>.</sub>
</p>

<p align="center">
  <img src="assets/demo.gif" width="920" alt="Tour tính năng trên terminal chạy với một index ERPNext thật: reify index dựng lại đồ thị; reify context biên dịch bản tóm tắt cho việc thêm bậc chiết khấu dưới ngân sách 1.500 token; reify why trên một dòng của customer.py trả về nơi gọi nó, các bảng nó ghi, các file thường đổi cùng nó và những commit 2022-2025 giải thích nó; reify impact truy vết bán kính ảnh hưởng của check_credit_limit qua nhiều bước; reify explain cho thấy khái niệm hạn mức tín dụng trên mọi file nó xuất hiện; và reify context --toon xuất cùng dữ kiện đó ở định dạng dành cho agent.">
</p>

<p align="center">
  <sub>Mọi lệnh trong đoạn demo đều là thật, chạy trên một index ERPNext thật. Kịch bản ghi hình đã được <a href="assets/demo.tape">commit</a> (ghi bằng <a href="https://github.com/charmbracelet/vhs">vhs</a>); nếu ảnh động có bao giờ mâu thuẫn với công cụ, hãy ghi lại ảnh động.</sub>
</p>

## <a id="two-minutes"></a>Hai phút đến câu trả lời đầu tiên

```bash
curl -fsSL https://raw.githubusercontent.com/lambiengcode/reify/main/install.sh | sh
cd your-repository
reify init --write-agent-instructions   # nối agent của bạn qua AGENTS.md / CLAUDE.md
reify index                             # 4,6 giây cho 5.000 file; 0,7 giây sau một lần sửa
reify context "thay đổi bạn sắp thực hiện" --toon
```

<sub>Một binary tĩnh duy nhất — không daemon, không config, không API key, và mỗi bản
phát hành kèm checksum SHA-256 mà <code>reify upgrade</code> kiểm tra trước khi cài.
Đổi ý? <code>reify uninstall</code> xoá binary, <code>reify uninit</code> dọn một
repository, cả hai đều in kế hoạch trước. Nối từng agent, hook và MCP: <a href="#install">Cài đặt</a>.</sub>

<p align="center">
  <a href="README.md">English</a> &middot; <strong>Tiếng Việt</strong> &middot; <a href="README.zh.md">简体中文</a>
</p>

---

**Mục lục**

- [Hai phút đến câu trả lời đầu tiên](#two-minutes) · [vấn đề phụ thuộc một người](#the-one-person-problem) · [nó cho bạn cái gì](#what-it-actually-gives-you)
- **Số liệu:** [SWE-bench Verified](#swebench) · [bốn repository](#numbers) · [chỗ nó không chạy được](#where-it-doesnt-work)
- **Dùng nó:** [cài đặt](#install) · [nối vào agent](#other-agents) · [các lệnh](#commands) · [quyền riêng tư](#privacy)
- **Bên dưới:** [cách hoạt động](#how-it-works) · [nó đọc gì](#what-it-reads) · [đa ngôn ngữ](#multilingual) · [kiến trúc](#architecture)
- [Câu hỏi thường gặp](#faq) · [phát triển](#development) · [lộ trình](#roadmap) · [giấy phép](#license)

## <a id="the-one-person-problem"></a>Vấn đề phụ thuộc một người

Hệ thống của bạn đã mười một năm tuổi. Logic nghiệp vụ thì khổng lồ và gần như không
có tài liệu — có vài tài liệu BA nằm trên SharePoint từ 2019, và một số trong đó vẫn
còn đúng.

**Chỉ một người hiểu nó.** Người đó không thể đi nghỉ mà không mang theo điện thoại.
Bạn không thể tuyển người để giảm tải, vì một dev mới cần gần một năm mới dùng được,
mà những kiến thức họ cần hấp thụ thì chẳng được viết ở đâu cả — nó nằm trong đầu một
người, và người đó thì quá bận để ngồi viết ra.

Thế là bạn chĩa một AI coding agent vào đó. Agent giỏi xuất sắc với code mới và vô
dụng ở đây. Nó đọc nhầm bốn mươi file, bỏ sót đúng cái luật quan trọng, rồi tự tin sửa
một hành vi mà khách hàng đang phụ thuộc vào. Sau đó vẫn đúng người kia phải ngồi
review — chính là cái nút thắt bạn đang muốn gỡ.

**Reify lấy kiến thức đó ra khỏi một cái đầu và đưa vào dạng mà cả agent lẫn người mới
đều dùng được — mà không bắt ai viết tài liệu mà họ sẽ không bao giờ viết.** Nó biên
dịch những gì đã có sẵn: code, những tài liệu BA không ai đọc, schema cơ sở dữ liệu, và
mười một năm commit message giải thích *tại sao*.

### Codebase của bạn nghe có giống thế này không?

- [x] Già hơn người mới nhất trong team
- [x] Luật nghiệp vụ nằm rải rác trong code, stored procedure, config và trí nhớ của ai đó
- [x] Không có tài liệu cho dev. Có vài tài liệu BA dạng Word hoặc PDF, không rõ còn đúng đến đâu
- [x] "Hỏi anh Minh ấy, ảnh viết chỗ đó" là một câu trả lời bình thường cho câu hỏi kỹ thuật
- [x] Onboarding tính bằng tháng
- [x] Tài liệu *hiện có* thì mâu thuẫn với code, và không ai biết mâu thuẫn ở chỗ nào
- [x] AI agent chạy ngon trên side project của bạn và sụp đổ trên hệ thống này
- [x] Source code không được phép rời khỏi công ty

Reify được xây đúng cho tình huống này. Nếu không điều nào nghe quen, có lẽ bạn không
cần nó — xem [Câu hỏi thường gặp](#faq).

## <a id="what-it-actually-gives-you"></a>Reify thực sự cho bạn cái gì

Ba câu hỏi, được trả lời từ bằng chứng chứ không phải từ trí nhớ của một mô hình:

| Câu hỏi | Lệnh | Ai hỏi |
|---|---|---|
| *Tại sao đoạn code này tồn tại?* | `reify why <file>:<line>` | người mới, ngày thứ hai đi làm |
| *Sửa cái này thì hỏng cái gì?* | `reify impact "<symbol>"` | người trực tiếp sửa |
| *Tôi cần biết gì trước khi bắt đầu?* | `reify context "<task>"` | **AI agent của bạn, mọi lượt** |

Câu thứ ba mới là câu quan trọng. Nó đưa cho agent tập nhỏ nhất gồm luật, trích dẫn
nguồn, đoạn code và những mâu thuẫn đã biết mà agent cần — và không gì khác.

### Cho người mà cả hệ thống đang phụ thuộc vào

Bạn không phải viết tài liệu. Reify đọc những gì đã có sẵn, và ở chỗ nào nó phải đoán,
`reify concepts --suggest` đưa bạn một bản nháp từ điển thuật ngữ để sửa trong một
buổi chiều, thay vì phải soạn từ con số không. Mười phút bạn ngồi sửa còn giá trị hơn
cả tuần khai quật của người khác.

### Cho người vừa mới vào

```bash
reify report                       # rốt cuộc mình đang nhìn cái gì đây
reify explain "hạn mức tín dụng"   # trong mọi ngôn ngữ nó xuất hiện
reify flow "duyệt đơn hàng"        # đường đi của code, theo đúng thứ tự
reify conflicts                    # chỗ nào tài liệu đang nói dối mình
```

## <a id="before--after"></a>Trước / sau

Bạn nhờ agent đổi ngưỡng duyệt đơn hàng. Nó grep `50000000`, thấy đúng một chỗ, sửa, và ship. Nó không bao giờ biết được rằng BRD nói khách hàng doanh nghiệp thì *luôn* phải duyệt, trong khi code đã lặng lẽ bỏ qua bước đó từ 2019.

Với reify:

```
$ reify why erpnext/selling/doctype/sales_order/sales_order.py:812

  [CONFLICT] documentation and implementation disagree about approval
    documented   Corporate customers must require approval    docs/BRD-42.md:6
    observed     Corporate customers bypass approval          sales_order.py:812

  Called by     3 services, 1 batch job
  Writes        tabSales Order, approval_log
  History       8a31c2f  2019-04-17  fix: enterprise approval flow
```

Ba trong bốn phần đó là những thứ grep về mặt cấu trúc không thể tạo ra.

## <a id="swebench"></a>Số liệu, trên một benchmark không phải của chúng tôi

Benchmark bốn repository bên dưới là của chúng tôi, và đó chính là lý do phải chạy thêm
benchmark của người khác. **[SWE-bench Verified](https://openai.com/index/introducing-swe-bench-verified/)**
gồm 500 issue GitHub thật từ mười hai dự án Python nổi tiếng, mỗi issue được ghim vào
đúng `base_commit` mà nó được báo cáo — cùng một giao thức "index trước khi thay đổi"
mà benchmark của Reify dùng, nhưng do người khác viết ra. Đề bài là một báo cáo lỗi
thông thường; đáp án đúng là tập file mà bản vá được chấp nhận thực sự đã sửa.

| truy xuất trên SWE-bench Verified, n=500 | đề xuất được một file mà bản vá đã sửa | MRR | đề xuất được **mọi** file như vậy | token trung vị |
|---|--:|--:|--:|--:|
| grep, content | 6.6% <sub>[4.7–9.1]</sub> | 0.06 | 5.6% | 3,998 |
| grep, paths | 9.0% <sub>[6.8–11.8]</sub> | 0.06 | 7.8% | 3,996 |
| **reify**, một vòng | **66.0%** <sub>[61.7–70.0]</sub> | 0.43 | 59.0% | **3,466** |
| **reify**, ba vòng | **84.6%** <sub>[81.2–87.5]</sub> | 0.45 | 77.0% | 9,174 |

**Một vòng Reify duy nhất thắng grep trên 310 instance và thua 13 — trong khi tiêu ít
token hơn** (3.466 so với 3.998). Ba vòng thắng 395–5 (McNemar chính xác
p ≈ 7 × 10⁻¹¹⁰). Đây không phải phép đo sát nút, và nó là con số sạch nhất trong tài
liệu này chính bởi vì đề bài, repository lẫn đáp án đều đến từ nơi khác.

Theo từng repository, ba vòng so với grep nội dung:

| | grep | reify ×3 | | | grep | reify ×3 |
|---|--:|--:|---|---|--:|--:|
| django (n=231) | 6% | **88%** | | astropy (n=22) | 0% | **77%** |
| sympy (n=75) | 7% | **77%** | | xarray (n=22) | 9% | **91%** |
| sphinx (n=44) | 7% | **75%** | | pytest (n=19) | 26% | **84%** |
| matplotlib (n=34) | 0% | **91%** | | pylint (n=10) | 10% | **60%** |
| scikit-learn (n=32) | 9% | **88%** | | requests (n=8) | 0% | **100%** |

**Điều này chứng minh gì và không chứng minh gì.** Nó đo *truy xuất* — liệu những file
cần sửa có được đặt trước mặt mô hình hay không — chứ không đo việc mô hình sau đó có
viết đúng bản vá hay không. Verified chỉ có Python, nên nó không nói gì về điểm yếu
TypeScript hiện đại [bên dưới](#where-it-doesnt-work). Và các repository này nổi tiếng
đến mức mô hình đã thuộc lòng một phần; điều đó ảnh hưởng tới *câu trả lời* của mô
hình, không ảnh hưởng tới việc một bộ truy xuất đề xuất file nào, và mọi nhánh ở đây
đều chạy trên cùng một index tại cùng một commit. Tái lập bằng driver trong
[`benchmarks/swe/`](benchmarks/swe/).


### Đầu-cuối: hoà, và cái giá để đến được đó

Truy xuất không phải tuyên bố cuối cùng — giải quyết được issue mới là. Cùng benchmark
đó, chạy qua chính giao thức của bài báo SWE-bench (một mô hình, một ngân sách, bộ truy
xuất là khác biệt duy nhất), mọi bản vá do **bộ chấm chính thức của SWE-bench** phán quyết:

| giải quyết được issue, 63 instance chấm ở cả hai nhánh | | |
|---|--:|---|
| BM25 | 23,8% | |
| **Reify** | **23,8%** | 6–6, p = 1,0 |

Hoà — và cần nói thẳng, vì lần thử đầu tiên là **thua**: 11,1% so với 18,1%. Điều thú vị
nằm ở chỗ đã khép lại khoảng cách đó bằng cách nào.

Reify tìm đúng file thường xuyên hơn hẳn (77% so với 60% của BM25) mà mô hình vẫn làm tệ
hơn. Dựng lại đúng những prompt đó cho thấy lý do: một cửa sổ ngữ cảnh được đổ đầy bằng
nguyên cả file theo thứ tự xếp hạng sẽ tiêu hết vào thứ đứng đầu, và cách xếp hạng của
Reify mù trước kích thước file, trong khi BM25 có chuẩn hoá độ dài ngay trong công thức.
**File đúng đã được tìm ra rồi không bao giờ được cho xem** — chỉ hiện diện trong 27% số
prompt, so với 40% của BM25.

`reify context --for-edit` sửa từ gốc: các vùng được nới ra thành định nghĩa trọn vẹn,
phần import của file được đưa vào một lần, các vùng chồng nhau được gộp, ngân sách vẫn
cứng. Không còn mất gì ở cửa sổ nữa:

| | tìm được file đúng | **hiện diện trong prompt** |
|---|--:|--:|
| BM25 | 60,0% | 40,0% |
| Reify, nguyên file | 76,7% | 26,7% |
| **Reify `--for-edit`** | **80,0%** | **56,7%** |

Hai cách sửa đã bị **bác bỏ dựa trên bằng chứng**: giới hạn theo từng file làm mọi thứ tệ
hơn (một file bị cắt cụt thì cũng không sửa được), và xếp hạng theo chi phí làm giảm truy
xuất bảy điểm mà chẳng đem lại gì khi các vùng đã khiến kích thước file không còn quan trọng.

Vậy: Reify thắng rõ ràng ở truy xuất và hoà ở kết quả vá cuối cùng. Ràng buộc còn lại nằm
ở vòng lặp viết bản vá chứ không phải ở ngữ cảnh — cả hai nhánh đều chạm trần quanh 24%.

## <a id="numbers"></a>Số liệu, trên bốn repository chọn để làm khó


Phép đo trung thực là một mô hình thật làm một task thật: ticket lấy từ các commit đã
merge, trong đó prompt chính là mô tả thay đổi do dev tự viết, còn đáp án đúng là những
file họ thực sự đã sửa. **Mọi index đều được dựng tại một commit trước khi bất kỳ thay
đổi nào trong số đó tồn tại**, nên đoạn code đang được hỏi thật sự chưa có mặt. Bốn
repository, chọn một phần là để làm khó chính mình; nhiều điều kiện được thiết kế để
phá kết quả chứ không phải để ủng hộ nó.

<p align="center">
  <img src="assets/benchmark-agent.svg" width="860" alt="Tỷ lệ trúng theo từng điều kiện trên bốn repository, thanh râu là khoảng tin cậy 95%. ERPNext, 40 task: không có ngữ cảnh 22%, grep với ngân sách gấp ba 50%, reify ba vòng 75%, ngữ cảnh hoàn hảo 100%. OFBiz, 40 task: 0%, 28%, 78%, 100%. OpenMRS, 22 task: 0%, 32%, 59%, 100%. Medusa, 40 task: 0%, 24%, 26%, 100% — reify và grep chồng lấn trên Medusa.">
</p>

So sánh chính đã được cân bằng chi phí: Reify lặp ba vòng (một agent đọc, không thấy,
rồi hỏi lại — với những file đã đọc bị loại ra), nên nhóm đối chứng là grep được phát
thẳng đúng ngân sách gấp ba đó.

| có mô hình, tỷ lệ trúng | grep ×3 ngân sách | **reify ×3 vòng** | chênh lệch | khoảng tin cậy 95% có chồng nhau? |
|---|--:|--:|--:|---|
| ERPNext (Python/JS), n=40 | 50% | **75%** | +25 | sát nút |
| OFBiz (Java + XML), n=40 | 28% | **78%** | +50 | không |
| OpenMRS (Java), n=22 | 32% | **59%** | +27 | sát nút |
| Medusa (TS hiện đại), n=40 | 24% | **26%** | +2 | **chồng hoàn toàn — không thắng** |

> **Lưu ý về hàng thứ tư:** +2 của Medusa là hoà, không phải thắng, và nó đứng trong
> bảng chính với đầy đủ vị trí thay vì bị giấu xuống chú thích. Điều gì phân tách các
> repository mà Reify thắng đậm với nơi nó không thắng đã được đo, không phải đoán —
> xem [Chỗ nó không chạy được](#where-it-doesnt-work).

**Các nhóm đối chứng, trên mọi repository:** ngữ cảnh hoàn hảo đạt 100% ở khắp nơi, nên
chất lượng truy xuất chính là toàn bộ cuộc chơi. Ngữ cảnh mồi nhử có hình dạng y hệt
chỉ đạt 0–12%, nên thứ tạo ra kết quả là nội dung chứ không phải định dạng. Khi không
được truy cập repository, mô hình đạt 0% ở ba repository và **22% ở ERPNext** — nó đã
thuộc lòng một phần cái repo nổi tiếng nhất, và đó chính là lý do ba repository kia tồn
tại, cũng như lý do mọi con số "khoảng cách còn lại" đều trừ đi cái nền này. Bảy trong
khoảng 1.000 lượt gọi tới nhà cung cấp mô hình bị lỗi, tất cả đều ở Medusa; các lượt
lỗi bị loại khỏi tỷ lệ, không bao giờ bị tính thành trượt.

Một vòng duy nhất, để ghi nhận: reify 55/68/41/15 so với grep 30/12/41/21 ở cùng ngân
sách một vòng — riêng OFBiz, *một* vòng reify đã hơn grep 56 điểm.

### <a id="retrieval-alone"></a>Chỉ riêng truy xuất, không có mô hình

<p align="center">
  <img src="assets/benchmark-retrieval.svg" width="860" alt="Tỷ lệ task mà một file bị thay đổi được đề xuất, theo từng repository. ERPNext: grep 10%, grep theo đường dẫn 18%, reify 57%, reify ba vòng 75%. OFBiz: 12%, 15%, 70%, 78%. OpenMRS: 32%, 18%, 41%, 55%. Medusa: 18%, 18%, 18%, 28%.">
</p>

| file bị thay đổi có được đề xuất | grep | reify (MRR) | **reify ×3** |
|---|--:|--:|--:|
| ERPNext | 10% | 57% (0.45) | **75%** |
| OFBiz | 12% | 70% (0.45) | **78%** |
| OpenMRS | 32% | 41% (0.27) | **55%** |
| Medusa | 18% | 18% (0.09) | **28%** |

### <a id="the-scorecard"></a>Bảng điểm, so với mục tiêu đặt ra từ trước

Bảy mục tiêu đã được đăng ký trước khi bắt đầu đợt cải tiến. **Một trên bảy đạt được**
(đo trên bốn repository). Tỷ lệ trúng, phần khoảng cách thu hẹp được, độ chênh giữa các
repository, MRR, độ chính xác và tỷ lệ hoàn thành đầu-cuối đều chưa chạm ngưỡng. Phần
cải thiện là thật — các mục tiêu được đặt cao có chủ ý, và mục tiêu chưa đạt với số
liệu trung thực vẫn hơn mục tiêu đạt được với ngưỡng dễ dãi.

### <a id="where-it-doesnt-work"></a>Chỗ nó không chạy được

**Medusa** — một monorepo TypeScript hiện đại, phân tách tốt — là bài toán còn mở, và
nó lật ngược giả định nền tảng của dự án. Các hệ thống Java cũ lẽ ra phải là ca khó;
hoá ra chúng là ca *tốt nhất*. Task của Medusa mô tả hành vi giao diện ("bỏ nút đăng
nhập cloud bị trùng") với vốn từ gần như không giao với code, lịch sử thì là các PR
merge đã squash, và không thứ gì Reify đang đọc lấp được khoảng cách đó. Lặp vòng nâng
truy xuất từ 18 lên 28%; có mô hình thì 26% so với 24% của grep; các khoảng tin cậy
chồng lên nhau hoàn toàn.

Giả thuyết trước đó — "lợi thế của Reify tỉ lệ thuận với lượng từ vựng được khai báo" —
cũng không sống sót qua bài kiểm tra bốn repository. OFBiz khai báo rất ít theo kiểu
ERPNext làm, vậy mà lại cho mức chênh lớn nhất. Thứ thực sự phân tách bốn repository là
*lịch sử commit và cách đặt tên file có nói cùng thứ ngôn ngữ mà task được viết ra hay
không*. Ở đâu có, Reify thu hẹp được 54–62% khoảng cách tới ngưỡng lý tưởng. Ở đâu
không (Medusa), nó chỉ là grep có cấu trúc tốt hơn.

## <a id="how-it-works"></a>Cách nó hoạt động

**Xác định trước. Ngữ nghĩa sau. LLM cuối cùng.** Trong bản build này không hề có LLM nào trừ khi bạn tự cấu hình, và mọi lệnh vẫn chạy được khi không có.

```
1. Có trong AST không?          → symbol, lời gọi, import, kế thừa
2. Có trong tầng dữ liệu không? → bảng, cột, ánh xạ ORM, SQL nhúng
3. Có trong tài liệu không?     → mục, trích dẫn theo tiêu đề
4. Có trong git không?          → ai đưa vào, cái gì sửa nó, cái gì đi kèm nó
5. Có được khai báo ở đâu không? → từ điển thuật ngữ, metadata thực thể, file dịch
6. Chỉ đến lúc đó mới suy diễn  → và đánh dấu INFERRED, kèm bằng chứng
```

Mọi khẳng định đều mang theo nguồn gốc và mức độ đáng tin của nó:

| | |
|---|---|
| `CONFIRMED` | đọc thẳng từ một file nguồn |
| `OBSERVED` | suy ra một cách xác định từ các dữ kiện đã xác nhận |
| `INFERRED` | một heuristic. **Kiểm tra trích dẫn nguồn trước khi hành động theo nó** |
| `CONFLICTED` | hai nguồn mâu thuẫn nhau. Giải quyết trước khi đổi hành vi |
| `UNKNOWN` | chưa xác định một cách tường minh, nên sự vắng mặt không bao giờ bị coi là bằng chứng |

`Status::Unknown` là giá trị `Default` một cách có chủ ý. Bất cứ thứ gì quên khai báo chỗ đứng của mình sẽ rơi vào đúng trạng thái mà agent không được phép hành động theo.

### <a id="four-bridges"></a>Bốn cây cầu từ ngôn ngữ nghiệp vụ sang code

Xếp theo độ chính xác giảm dần. Cây cầu cuối là thứ khiến Reify vẫn chạy được trên một repository không khai báo gì cả.

| Cây cầu | Nguồn | Có được khi |
|---|---|---|
| **Khai báo** | `.reify/glossary.toml`, metadata thực thể, ánh xạ ORM | có người hoặc framework đã viết ra |
| **Bản dịch** | bảng i18n, message bundle | sản phẩm đã được bản địa hoá |
| **Đồng xuất hiện** | tiêu đề tài liệu có gọi tên code | có tài liệu |
| **Từ vựng của code** | những cụm từ mà các định danh cứ lặp đi lặp lại | **luôn luôn** |

Cây cầu cuối chỉ chạy trên phần mà ba cây cầu kia chưa phủ, nên nó lấp chỗ trống thay vì cạnh tranh với bằng chứng tốt hơn. Boilerplate bị lọc bằng cách đo xem từ nào phổ biến khắp nơi *trong chính repository này*, chứ không dựa vào một danh sách `get`/`set`/`manager` soạn sẵn — thứ chỉ hợp với đúng một stack và không hợp với stack nào khác.

## <a id="what-it-reads"></a>Nó đọc được gì

**Code, 11 ngôn ngữ.** Python, TypeScript, JavaScript, Java, Go, C#, Rust, Ruby, PHP, C/C++, Kotlin, cộng thêm SQL. Mỗi ngôn ngữ có một test khẳng định nó cho ra container, hàm gọi được *và* lời gọi — bởi vì thiếu một node của grammar sẽ cho bạn một index trông thì khoẻ mạnh mà mỗi file chỉ có một symbol. Đó không phải giả định: nó đã từng xảy ra với Java, và giờ test bắt được.

**Tài liệu, viết kiểu gì cũng được.** Đây là phần mà hầu hết công cụ code bỏ qua, và
với nhiều hệ thống kiểu này thì nó là tài liệu duy nhất họ có.

| | |
|---|---|
| Đọc trực tiếp | Markdown, văn bản thuần, HTML (kể cả bản export từ Confluence) |
| Zip + XML | DOCX, ODT, XLSX, PPTX |
| Uỷ quyền | PDF, DOC nhị phân cũ, RTF |

Những định dạng không có thư viện Rust thuần nào đọc được sẽ đi qua trình chuyển đổi ngoài (`pdftotext`, `mutool`, `antiword`, `textutil`, `soffice`), thử lần lượt theo thứ tự. Khi không cài cái nào, Reify **liệt kê đúng từng công cụ đã thử và cách cài** thay vì lặng lẽ index rỗng.

**Bất cứ thứ gì team đã khai báo.** Frappe DocType JSON, ánh xạ Hibernate ORM, message bundle `.properties` của Java và Spring, bảng i18n dạng CSV. Đây là nguồn từ vựng chính xác nhất mà một repository có thể đưa ra, bởi vì chính ứng dụng đọc nó nên nó luôn đúng.

## <a id="multilingual"></a>Đa ngôn ngữ

Không ngôn ngữ nào là gốc, kể cả tiếng Anh. Id của khái niệm là mờ (opaque) và mọi nhãn đều mang thẻ ngôn ngữ, nên một yêu cầu viết bằng tiếng Việt, tiếng Thái, tiếng Hàn hay tiếng Đức vẫn chạm tới code tiếng Anh thông qua tầng khái niệm chứ không qua mô hình embedding — và đó là lý do câu trả lời đến kèm số dòng thay vì một điểm số tương đồng.

Khoảng 60 locale được nhận diện trên file dịch và message bundle. Ngôn ngữ diễn đạt nghĩa vụ và miễn trừ được phát hiện trong 11 thứ tiếng, nên một luật viết bằng bất kỳ ngôn ngữ nào trong đó vẫn được khai thác thành luật.

Ba thứ chỉ vỡ khi bạn rời khỏi hệ chữ Latinh, và cả ba đều đã vỡ ở đây trước:

- **Tiếng Thái, Lào, Khmer, Nhật và Trung không có dấu cách giữa từ**, nên index theo từ sẽ lưu một token khổng lồ và tìm một từ *bên trong* nó thì không khớp gì cả. Có một index trigram cho nội dung ngoài ASCII; repository thuần ASCII không bao giờ phải trả giá cho nó.
- **Tiếng Hàn dính tiểu từ vào gốc từ.** `승인` biến thành `승인을`, và so khớp nguyên từ không tìm ra cái nào cả.
- **Độ dài câu không thể đếm bằng dấu cách**, nếu không mọi yêu cầu tiếng Thái đều bị loại vì "quá ngắn để là một luật".

## <a id="install"></a>Cài đặt

```bash
curl -fsSL https://raw.githubusercontent.com/lambiengcode/reify/main/install.sh | sh
```

Có sẵn binary dựng trước cho macOS (Apple Silicon và Intel) và Linux (x86_64 và aarch64).
Hoặc build từ mã nguồn:

```bash
cargo install --path crates/reify-cli
```

Sau đó, trong bất kỳ repository nào:

```bash
reify init      # cho biết nó sẽ index cái gì, không index cái gì, và vì sao
reify index     # 4,6 giây cho 5.000 file; 0,7 giây sau khi bạn sửa một file
```

**Cập nhật gọn, gỡ bỏ sạch.** `reify upgrade` thay binary bằng bản phát hành mới nhất —
qua `curl` và `tar` dưới dạng tiến trình con nhìn thấy được, không bao giờ qua HTTP
client nhúng, và checksum được kiểm tra trước khi cài bất cứ thứ gì; `--check` chỉ hỏi,
còn `REIFY_OFFLINE=1` từ chối thẳng lệnh này. `reify uninstall --yes` xoá binary và
không gì khác; `reify uninit --yes` xoá kho `.reify/` của một repository cùng khối hướng
dẫn mà `init` đã ghi. Cả hai đều in kế hoạch trước khi chạy nếu thiếu `--yes`.

<details>
<summary><strong>Tự động hoàn thành lệnh trong shell</strong></summary>

```bash
reify completions zsh  > ~/.zfunc/_reify
reify completions bash > /etc/bash_completion.d/reify
reify completions fish > ~/.config/fish/completions/reify.fish
```

</details>

### <a id="claude-code"></a>Claude Code

Mức 0 — đúng mức mà benchmark đã đo, và cũng là mức nên bắt đầu:

```bash
reify init --write-agent-instructions
```

Lệnh đó thêm một khối sáu dòng vào `AGENTS.md` hoặc `CLAUDE.md` của bạn. Không giao thức, không server, không phải trả thuế schema mỗi lượt.

<details>
<summary><strong>Hook chạy trước khi sửa, và giữ cho index luôn mới</strong></summary>

Chèn một dòng cảnh báo rủi ro trước mỗi lần sửa file:

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

Dưới 300 token, có test khẳng định điều đó, vì nó chạy trên mọi lần sửa. Mặc định không chặn: một hook chặn thao tác sửa sẽ bị gỡ, và khi đó mất luôn cả những cảnh báo của nó.

Giữ index luôn mới:

```bash
printf '#!/bin/sh\nreify index >/dev/null 2>&1 &\n' > .git/hooks/post-merge
chmod +x .git/hooks/post-merge
cp .git/hooks/post-merge .git/hooks/post-checkout
```

</details>

### <a id="other-agents"></a>Codex, Cursor, OpenCode, Aider, Pi, Windsurf, Cline

Không cần adapter — Reify là một CLI. Bỏ đoạn này vào bất kỳ file hướng dẫn nào mà công cụ đó đọc (`AGENTS.md`, `.cursorrules`, `CONVENTIONS.md`, `.windsurfrules`, `.clinerules/`):

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

Ba công cụ — `reify_context`, `reify_why`, `reify_impact` — và ba là toàn bộ bề mặt. Schema của một MCP server bị gửi lại mỗi lượt của mỗi phiên, nên một công cụ sinh ra để tiết kiệm ngữ cảnh thì không nên thu tiền thuê chỗ để giao hàng. Một test khẳng định các schema tốn dưới 600 token.

### <a id="optional-a-model"></a>Tuỳ chọn: dùng một mô hình

Không có nhà cung cấp mặc định và không có gì được bật cho tới khi bạn cho phép.

```toml
# .reify/llm.toml
command = ["ollama", "run", "llama3"]
```

Reify ghi prompt vào stdin của lệnh đó, hoặc thay vào tham số `{prompt}`. Xem [Quyền riêng tư](#privacy) để biết vì sao đó là một lệnh chứ không phải một HTTP client.

## <a id="commands"></a>Các lệnh

| Lệnh | Nó làm gì |
|---|---|
| `reify context "<task>"` | Lượng kiến thức tối thiểu cho một thay đổi, kèm kế hoạch đọc. **Lệnh quan trọng nhất.** `--toon` xuất ra định dạng dành cho agent |
| `reify why <file>:<line>` | Đây là cái gì, ai gọi nó, nó đụng vào dữ liệu nào, cái gì đã sửa nó |
| `reify impact "<symbol>"` | Cái gì phụ thuộc vào nó — kể cả qua cơ sở dữ liệu, nơi không tồn tại cạnh lời gọi nào |
| `reify explain "<term>"` | Một khái niệm nghiệp vụ, xuyên qua mọi ngôn ngữ, bảng và file mà nó xuất hiện |
| `reify flow "<process>"` | Chuỗi lời gọi thực hiện một quy trình nghiệp vụ |
| `reify conflicts` | Chỗ tài liệu mâu thuẫn với code |
| `reify rules` | Các luật nghiệp vụ khai thác được, kèm bằng chứng |
| `reify concepts --suggest` | Biến thứ khai thác được thành các mục từ điển để bạn biên tập lại |
| `reify preflight <file>` | Dòng cảnh báo rủi ro cho hook của editor |
| `reify report` | Bảng điểm hệ thống |
| `reify status` | Độ mới, độ phủ, và những gì đã bị bỏ qua |
| `reify llm status \| preview` | Đã cấu hình mô hình chưa, và chính xác cái gì sẽ được gửi đi |
| `reify upgrade [--check]` | Thay binary này bằng bản phát hành mới nhất. Lệnh duy nhất dùng mạng; bị từ chối khi `REIFY_OFFLINE=1` |
| `reify uninstall --yes` \| `uninit --yes` | Xoá binary \| kho và khối hướng dẫn của một repository |
| `reify serve --mcp` | Model Context Protocol qua stdio |
| `reify completions <shell>` | Script tự động hoàn thành lệnh |

Mọi lệnh đều nhận `--json` theo một schema có đánh phiên bản và `--budget <tokens>`.
Xem đầy đủ cấu trúc đầu ra: [docs/json-schema/](docs/json-schema/).

**Agent nên yêu cầu `--toon`.** JSON lặp lại tên mọi trường ở mọi bản ghi; TOON khai báo
cột của mỗi mục đúng một lần, rồi mỗi bản ghi là một dòng — đo được **ít hơn 57% token
cho cùng một lượng thông tin**, với `status` vẫn là cột đầu tiên của mọi dòng. Phần
header mang theo chi phí token đo được của chính những byte đang được xuất ra, nên con
số ngân sách và nội dung không thể lệch nhau. `reify_context` của MCP vốn đã trả lời
bằng TOON.

## <a id="privacy"></a>Quyền riêng tư

**Mã nguồn và tài liệu nghiệp vụ của bạn không bao giờ rời khỏi máy.** Reify không mở
kết nối mạng nào — không phải "theo mặc định", mà là hoàn toàn không. Không có HTTP
client nào trong cây phụ thuộc, và `cargo test` sẽ làm hỏng build nếu có một cái xuất hiện.

Với một công ty không cho phép mã nguồn độc quyền đến gần dịch vụ đám mây, đó là khác
biệt giữa một công cụ họ có thể đánh giá và một công cụ họ không thể.

| | |
|---|---|
| Thư viện mạng trong `Cargo.lock` | khẳng định bằng không, kiểm tra trong CI |
| Socket trong mã nguồn | khẳng định bằng không, kiểm tra trong CI |
| Tiến trình con | `git`, các trình chuyển đổi tài liệu đã rà soát, và — riêng cho `reify upgrade` — `curl` và `tar`; mỗi cái đều được nêu tên trong một test |
| Code từ repo của bạn, bị thực thi | không bao giờ. tree-sitter phân tích cú pháp; nó không chạy code |
| Kho lưu trữ | `.reify/`, được `reify init` đưa vào gitignore |

Hỗ trợ từ mô hình là một lệnh do **bạn** cấu hình, không phải một client nhúng sẵn. Mô hình chạy cục bộ hoạt động ngay mà không cần thêm code, không có credential nào đi qua Reify, `reify llm preview` in ra chính xác từng byte trước khi bất cứ byte nào được gửi, và `REIFY_OFFLINE=1` khiến nó không thể với tới được bất kể file cấu hình nói gì.

Mô hình đe doạ đầy đủ, gồm cả những gì **không** được bảo vệ: [docs/privacy.md](docs/privacy.md).

## <a id="architecture"></a>Kiến trúc

Một file SQLite cho mỗi repository. Không graph database, không vector store, không daemon.

```
  LAYER 4  Synthesis    optional model, cached, always INFERRED        llm.rs
  LAYER 3  Selection    seed → spread → budget knapsack → render       context.rs
  LAYER 2  Semantics    concepts, rules, conflicts       concepts.rs · rules.rs
  LAYER 1  Structure    symbols, calls, tables, sections, commits  extract/ · gitlog.rs
  LAYER 0  Substrate    walk, classify, hash, store      discover.rs · store.rs
```

**Index tăng dần cho ra kết quả giống hệt từng byte so với build lại từ đầu**, được khẳng định bằng một property test áp dụng các chuỗi chỉnh sửa ngẫu nhiên rồi so sánh bản dump chuẩn hoá. Mỗi tầng sở hữu một tập cạnh riêng biệt và cơ chế vô hiệu hoá riêng của nó, và đó là điều làm cho khẳng định trên đúng. Chi tiết: [docs/architecture.md](docs/architecture.md).

### <a id="measured-performance"></a>Hiệu năng đo được

ERPNext, 5.064 file, laptop chip M 8 nhân.

| | đo được |
|---|--:|
| index đầy đủ, không dùng mô hình | 4,6 giây |
| index lại, không có gì thay đổi | 0,6 giây |
| index lại, sửa một file | 0,7 giây |
| `reify context` | 57 ms |
| `reify impact` | 0,2 ms |
| `reify why` | 205 ms — do gọi tiến trình con `git log -L`; khoảng 5 ms nếu bỏ nó |
| bộ nhớ đỉnh, index đầy đủ | 224 MB |
| dung lượng kho | 47 MB (33% của cây làm việc 144 MB) |

Một lần index đầy đủ từng mất **78 giây** cho tới khi index toàn văn được đánh khoá theo id của node. `uid` là `UNINDEXED` trong FTS5, nên `DELETE ... WHERE uid = ?` quét toàn bộ bảng một lần cho mỗi node — độ phức tạp bình phương, và vô hình cho tới khi từng tầng được bấm giờ riêng. Sửa một file từng mất **5,9 giây** cho tới khi các tầng chạy trên toàn repository học được cách bỏ qua khi đầu vào của chúng chắc chắn không đổi.

`REIFY_TIMING=1 reify index` in ra bảng phân rã theo từng tầng đã tìm ra cả hai lỗi trên.

## <a id="reproducing-the-benchmark"></a>Tái lập benchmark

Không con số nào trong các bảng phía trên được gõ tay. Tập task, kết quả thô của từng task và các biểu đồ đều đã được commit.

```bash
# 1. Đóng băng một tập task từ các commit đã merge thật, kết thúc trước một mốc đã chọn
reify-bench tasks --repo <repo> --after <base-sha> --out benchmarks/tasks/mine.json

# 2. Index tại mốc đó, để thay đổi đang được hỏi thật sự chưa tồn tại
git worktree add /tmp/base <base-sha>
reify -C /tmp/base init && reify -C /tmp/base index

# 3. Truy xuất, rồi chạy với mô hình
reify-bench run   --repo /tmp/base --tasks benchmarks/tasks/mine.json --out results/
REIFY_LLM_COMMAND='<your model cli> {prompt}' \
reify-bench agent --repo /tmp/base --tasks benchmarks/tasks/mine.json --out results/

# 4. Báo cáo và biểu đồ, sinh ra từ kết quả thô
reify-bench report --in results/ --out benchmarks/REPORT.md
reify-bench chart  --results "Mine=results/" --out assets/
```

Tập task được đóng băng trước khi bất kỳ điều kiện nào chạy. Báo cáo có hẳn một mục **"Where Reify lost"** liệt kê mọi task mà baseline thắng, và đó là phần bắt buộc của tài liệu chứ không phải tuỳ chọn.

## <a id="development"></a>Phát triển

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo bench -p reify
```

Tất cả đều được ép chạy trong CI, kèm một lượt chạy test đầy đủ trong môi trường chặn mọi lưu lượng ra ngoài. `cargo test` bao gồm `crates/reify/tests/offline.rs`, sẽ làm hỏng build nếu một thư viện mạng lọt vào cây phụ thuộc.

Fixture nằm ở [`fixtures/minierp`](fixtures/) — một hệ thống nghiệp vụ nhỏ với kiến thức được *cài sẵn*: một luật có tài liệu, code mâu thuẫn với nó, một magic number, một khái niệm song ngữ, và sự phụ thuộc chéo module chỉ tồn tại qua một bảng dùng chung. Mọi khẳng định Reify đưa ra về nó đều có đáp án đúng đã biết, nên một lần sai ở đó là không thể chối cãi.

Thêm một ngôn ngữ gồm: một grammar, một bảng ánh xạ loại node, một nhánh trong `classify` và một golden test. Xem [CONTRIBUTING.md](CONTRIBUTING.md) để biết những quy tắc thiết kế mang tính chịu lực chứ không phải chuyện hình thức.

## <a id="faq"></a>Câu hỏi thường gặp

**Bọn mình chẳng có tài liệu kỹ thuật nào cả. Chỉ có tài liệu BA, và chúng cũ rồi.**
Đó chính là tình huống Reify được xây cho. Nó đọc DOCX, PDF, XLSX và các định dạng
khác, cắt chúng thành những mục có thể trích dẫn, và — quan trọng nhất — chỉ cho bạn
chỗ chúng *mâu thuẫn* với code, nhờ đó một tài liệu cũ trở thành bằng chứng thay vì cái
bẫy. Nếu hoàn toàn không có tài liệu nào, nó lùi về dùng chính vốn từ của code, và vẫn
cho bạn `why`, `impact` và lịch sử.

**Chuyên gia duy nhất của bọn mình không có thời gian hỗ trợ cài đặt.**
Không cần họ. `reify init && reify index` không cần gì từ họ cả. Nếu mượn được một buổi
chiều, `reify concepts --suggest` biến thứ Reify khai thác được thành bản nháp từ điển
để họ sửa thay vì phải soạn — và mục [Số liệu](#numbers) cho thấy từ vựng được khai báo
chính là chỗ tạo ra phần lợi ích.

**Cái này có thực sự giúp bọn mình tuyển được người không?**
Nó gỡ đúng một nút thắt: một dev mới, hoặc một agent, không thể tự tìm ra *tại sao* code
lại như vậy mà không phải làm phiền ai đó. Đó là một phần có thật của quá trình lên tay,
nhưng không phải toàn bộ. Ai nói rằng một công cụ thay thế được mười một năm ngữ cảnh
thì người đó đang bán hàng.

**Tôi có bắt buộc phải viết từ điển thuật ngữ không?**
Không, và Reify vẫn chạy khi không có. Một từ điển được khai báo vẫn là nguồn từ vựng
chính xác nhất bạn có thể đưa cho nó — `reify concepts --suggest` viết bản nháp đầu tiên
để bạn cắt gọt — nhưng benchmark bốn repository cho thấy yếu tố dự báo mạnh hơn là lịch
sử commit của bạn có nói cùng thứ ngôn ngữ với ticket của bạn hay không. Nếu team bạn
viết commit message tử tế, Reify đang đọc sẵn mười một năm ví dụ có nhãn rồi.

**Đây lại là một thứ RAG nữa à?**
Không có vector database, không có mô hình embedding và không có chunking. Truy xuất dựa trên từ vựng và đồ thị, và đó là lý do mọi câu trả lời đến kèm số dòng thay vì một điểm số tương đồng.

**Repo của tôi có 3.000 dòng. Tôi có nên dùng không?**
Không. Dùng ripgrep đi. Dưới khoảng 20 nghìn dòng code, Reify không cho bạn thứ gì mà grep và con lăn chuột không cho.

**Nó có gửi mã nguồn độc quyền của tôi đi đâu không?**
Không thể. Trong binary không có HTTP client nào, và một test sẽ làm hỏng build nếu có một cái xuất hiện. Nếu bạn cấu hình nhà cung cấp mô hình, đó là lệnh do bạn chọn, và `reify llm preview` cho bạn xem chính xác từng byte trước.

**Sao `reify why` chậm hơn mọi lệnh khác?**
Nó gọi ra `git log -L` để lấy lịch sử theo dòng chính xác. 205 ms khi có, khoảng 5 ms khi không. Vẫn nằm trong danh sách cần cải thiện.

**Lệnh conflicts không tìm thấy gì trong repo của tôi. Nó hỏng à?**
Chắc là không. Việc phát hiện đòi hỏi năm điều kiện cùng đúng một lúc và được thiên lệch mạnh về phía im lặng, bởi vì một bộ phát hiện mâu thuẫn hay báo động giả sẽ bị tắt ngay tuần thứ hai và mang theo cả những cảnh báo đúng của nó. Nó tìm thấy 0 trên ERPNext — repo gần như không có văn bản đặc tả — và đúng 1 trên fixture, nơi có một cái được cài sẵn.

**"Reify" nghĩa là gì?**
Là biến một thứ trừu tượng thành cụ thể. Kiến thức vốn luôn ở đó; chỉ là nó chưa từng là một file.

## <a id="roadmap"></a>Lộ trình

Đợt cải tiến đầu tiên đã xong. Tiên nghiệm từ lịch sử (mỗi commit đã merge là một ví dụ
có nhãn: message ≈ ticket, file thay đổi = đáp án), cạnh nối test với code, tinh chỉnh
lặp vòng và một repository thứ tư đều đã lên; phần fit trọng số trượt khâu kiểm định
trên tập giữ riêng và đã được khôi phục về mặc định đúng theo cam kết đăng ký trước; và
bảng điểm dừng ở một trên bảy mục tiêu, mỗi con số được in ngay cạnh ngưỡng của nó. Bài
toán còn mở là trường hợp TypeScript hiện đại, nơi chưa có gì lấp được khoảng cách từ
vựng giữa cách người ta mô tả thay đổi giao diện và cách code viết ra chúng.

## <a id="status"></a>Trạng thái

Còn sớm, và có đo đạc. Những điểm chưa đạt, đều được ghi rõ chứ không giấu: kho lưu trữ chiếm 33% cây làm việc so với mục tiêu 5%, `reify why` mất 205 ms so với mục tiêu 20 ms, và Windows chưa được kiểm thử.

## <a id="license"></a>Giấy phép

[Apache-2.0](LICENSE). Có kèm cấp quyền sáng chế, nên một nhà cung cấp agent thực sự có thể ship nó.
