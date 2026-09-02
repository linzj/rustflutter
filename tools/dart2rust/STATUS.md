# dart2rust 进度

目标:把 `/d/linzjUbuntu2204/gallery_upstream`(真正的 flutter/gallery)翻译成
Rust 跑起来。这需要翻译 gallery 自身的 Dart **和**它依赖的整个 Flutter framework。

尺子是 `bin/census.dart`:对一棵树跑前端,把拒绝原因**按类别归并**后排队。
队头就是下一件该做的事。用法:

    dart run --packages="E:/source/flutter/.dart_tool/package_config.json" \
        tools/dart2rust/bin/census.dart <目录> [--examples]

---

## 第 2 轮:具名参数

`painting/` 普查队头是 **具名参数 103 次**,远超第二名。

### 为什么这件事有静默出错的余地

Rust 没有具名参数,所以 Dart 的具名调用必须压平成位置调用。
**按调用点顺序压平**在绝大多数调用上都是对的——因为大多数人本来就按声明顺序写。
但 Dart 允许 `Rect.fromLTRB(top: a, left: b)`,这时两个参数会**静默互换**。

所以顺序必须来自**被调方的形参表**。先验证这一点可达:
`edge_insets.dart` 的 **130 个调用点,130 个都拿到了被调方的形参表**。

省略的可选参数也必须补上值,因为生成的 Rust 函数每个参数都是位置的:

- 有字面量默认值 → 按源码文本降下来
- 省略且可空 → `None`,这正是 Dart 给它的值
- 其余 → **停下**。编译器猜出来的默认值和上游写的默认值长得一模一样,
  这是最难在后期发现的一类错。

### 测试要能分辨"对"和"看着对"

fixture `testdata/fixtures/named_args.dart` 的调用**故意不按声明顺序写**,
权重取 10 的幂,好让任何错误排列都落不到正确的和上。

这个 fixture 当场抓出一个 bug:`allDefaults()`(一个具名参数都没给)
走了 `named.isEmpty` 的捷径,**三个默认值一个都没填**,生成了 `weigh()`。
捷径本身是错的——被调方已知时,每个参数都要交代。

变异验证,两个都被杀死:

| 变异 | 结果 |
|---|---|
| 具名参数按调用点顺序取 | 2 个测试失败 |
| 恢复 `named.isEmpty` 捷径 | 生成的 Rust 元数不匹配,rustc 拒绝 |

第二个是 rustc 报出来的,不是我的断言——记准确:它是真杀,但杀它的是编译器。

### 结果

`painting/` 普查:**具名参数 103 → 0**,翻译出的成员 268 → 329。
测试 9 个全过。

---

## 第 3 轮:队头是虚的,然后 `assert`

### 先查上一轮记的疑问,结果是记账 bug

`_lowerMethod` 里私有检查排在**最后**:

    if (member.isAbstract) throw ...;   // 先
    if (member.isSetter) throw ...;
    if (name.startsWith('_')) return;   // 后

所以 `double get _x;`(私有**且**抽象)被记成 `abstract method`,
`set _foo(v)` 被记成 `setter`。私有成员应该是**跳过**,不是拒绝——
库外没人能叫它的名字。字段那边是同一个形状:私有 `static final` 被记成
`non-const static field`。

改成私有优先后:**`abstract method` 74 → 57**,17 个是假的。

**队头有一部分是虚构的,会把下一轮送去建错东西。** 这就是为什么要先查。

### 真队头:`assert`(65)

Dart 的 `assert` 和 Rust 的 `debug_assert!` 是同一件事——debug 生效、release 编译掉。
这种对应精确到可以**翻译**而不是模拟。用 `assert!` 会把上游每一条检查
都留在 release 二进制里,那是这个编译器没资格替人做的性能决定。

三个判断:

- **消息是诊断,不是契约。** assert 的含义是它的条件,firing 与否和消息无关。
  所以纯字符串消息带过去,插值/调用的消息**保留源码在注释里**而不翻译——
  翻译它会把字符串格式化连同消息里调用的一切,拖进整棵树里每条 assert 的依赖集,
  只为了改变"出错之后"debug 构建打印什么。
- **带 assert 的构造函数不能再是 `const fn`。** 保 `const` 就得丢检查,
  这里选择保检查。
- 转义:反斜杠必须在双引号**之前**加倍,否则刚加的那个反斜杠会被下一步再加倍。

### 一个 bug 和它的两个同类

生成的消息一开始每个字符前面都多了反斜杠——`replaceAll(r'', ...)` 是**空模式**,
Dart 会在每个字符之间插入替换串。修的时候发现**字符串字面量那里是同一个 bug**
(根本没转义),一并修了。写这份记录时又栽了第三次(Python 的 `\u`)。
**转义要写在能看见它的地方,不要写在 heredoc 里穿三层。**

### 测试:每条都要有能触发它的搭档

否则"assert 翻译过来了"和"assert 被静默丢掉了"是同一个观察——
所有不触碰条件的测试两种情况下都通过。

变异三个全杀:

| 变异 | 结果 |
|---|---|
| assert 求值但不检查 | 3 个测试失败 |
| 字面量消息被丢掉 | 2 个失败 |
| 构造函数的 assert 不生成 | 1 个失败 |

### 结果

`assert` 语句 65 → 0,初始化列表 12 → 0,成员 296 → 305,测试 13 个全过。

**几项计数上升了**(后缀 41→46、super 40→43、级联 13→18、插值 35→38):
原先那些成员在 assert 处就被拒了,现在 assert 过去了,同一成员里的
**下一个**障碍才浮现。这是真进展,不是退步。

---

## 第 4 轮:命名构造函数,以及第一次拿真上游代码当测试

Dart 的命名构造函数和 Rust 的关联函数是**同一个形状**——
`EdgeInsets.all(8)` 和 `EdgeInsets::all(8.0)` 是同一个调用,
无名的那个按 Rust 惯例叫 `new`。什么都不用编码,所以什么都没编码。

先查了上游怎么写的:`EdgeInsets` 的四个命名构造函数全是**直接字段初始化**,
没有重定向。第 3 轮做的 assert 和字段初始化列表正好直接用上。

拒绝的三种,各有各的理由:

- **factory** —— 可能返回缓存实例或子类,不是"构造 Self 的关联函数",要类层次
- **重定向**(`: this.fromLTRB(...)`)—— 可以降成对另一个构造函数的调用,
  但那个构造函数的字段初始化还够不着,所以停下,而不是生成一个**不设任何字段**的构造函数
- **super 构造函数调用** —— 同样要类层次

### 拿真的 EdgeInsets 当测试,而不是合成 fixture

代价是要写 `Radius`/`RRect`/`ViewPadding` 的桩(约 30 行)。
回报是**当场抓到一个合成 fixture 抓不到的编译器 bug**。

`copyWith` 里的 `left ?? this.left` 生成了 `left.unwrap_or(*self.left)`——
`*self.left` 会解析成 `*(self.left)`,解引用的是字段不是接收者。

根因:**`this` 在 Rust 里按位置有两个身份**。
当**值**用时是 `*self`(结构体的一份拷贝),这是 `return this;` 要的;
当字段或调用的**接收者**用时必须是 `self`。现在由 `_receiver()` 统一决定。

桩这边也暴露了一件事,记下但不在本轮做:
上游 `Rect` 的 `right`/`bottom`/`width`/`height` 是 **getter**,
而译出来的代码按**字段**读。桩里先做成字段对付过去。
**Dart getter → Rust 方法**是一件真事,该有自己的一轮。

### 变异:四个全杀,但要分清是谁杀的

| 变异 | 结果 | 杀手 |
|---|---|---|
| 所有构造函数都生成成 `new` | DOES NOT BUILD | rustc |
| 只生成第一个构造函数 | DOES NOT BUILD | rustc |
| `this` 当接收者时退回 `*self` | DOES NOT BUILD | rustc |
| **字段初始化式整体轮转一位** | **4 个测试失败** | **断言** |

前三个都是 rustc 抓的,那说明我的断言在这三处没出力。
所以补了第四个:它**类型完全正确**(全是 f32),编译得过,
只是每个字段拿了下一个字段的初始化式。它挂了 4 个测试——
断言确实在检查**值**,不只是在检查"能编译"。

### 结果

`EdgeInsets` 拒绝 **13 → 5**,`painting/` 命名构造函数 62 → 0,测试 **20 个全过**,
其中 7 个跑的是**真的上游 EdgeInsets**。

---

## 第 5 轮:类层次第一刀——抽象类变 trait

上一轮末尾量出来:`abstract`(778)+ `super`(692)+ super 构造(212)+ `is`(182)
+ factory(115)= 全 framework 26% 的拒绝,**而且是同一件事**。本轮动它。

### 前提:编译器必须一次看整个文件

要生成 `impl AlignmentGeometry for Alignment`,得知道
`AlignmentGeometry` 是抽象的、它要求哪些方法——
这两件事**从 `Alignment` 内部都看不见**。
所以加了 `IrLibrary` 和 `lowerLibrary`,驱动多了 `--all` 模式。

### 映射:Dart 已经做好的区分,Rust 用同样的方式表达

- 抽象类 → **trait**(Rust 里"一组操作、自己不持有存储"就是这个)
- 抽象成员 → trait 的**必需方法**
- 有实现的成员 → trait 的**默认方法**

Dart 自己就是按"有没有函数体"分的,Rust 也是。**什么都不用发明。**

带不过来的是**字段**:Dart 的抽象类可以声明存储,Rust 的 trait 不行。
这种字段**在输出里报出来**,不是悄悄丢掉。

抽象类当**类型**用时是 `dyn Trait`——不是风格问题:
`fn add(other: AlignmentGeometry)` 根本编译不了,
Rust 不知道 `AlignmentGeometry` 有多大。所以拥有时 `Box<dyn>`、借用时 `&dyn`。

### 协变返回:两种语言真正不同的地方

Dart 允许 `Alignment operator -()` 覆盖返回 `AlignmentGeometry` 的方法。
Rust 要求 impl **精确**返回 trait 声明的类型。

解法是**委托**:`Alignment` 保留它自己返回 `Alignment` 的 `impl Neg`
(这是 Rust 调用者想要的),trait 方法调它再装箱。
一个函数体,两个返回类型,不用把 body 生成两遍。

### 跨轮次冲突,以及我第 3 轮判断错了

真代码撞出来的:`TextAlignVertical` 构造函数里有 assert,
又有由它构造的 `static const` 字段。第 3 轮我说
"带 assert 的构造函数不能是 `const fn`,保 const 就得丢检查"——

**这句话是错的。** const panic 从 Rust 1.57 起就稳定了,
`const fn` 里的 `debug_assert!` 编译得过,而且运行时照样触发。
我单独写了个最小例子验的。**两个从头到尾都能保住**,那次取舍根本不必要。
这一轮改回来了,并且这个错只在两轮的规则在真代码上相遇时才暴露。

### 变异:五个,四杀一等价

| 变异 | 结果 |
|---|---|
| 抽象类当成 struct 生成 | DOES NOT BUILD |
| 不生成 base 的 impl | DOES NOT BUILD |
| `_matching` 永远返回 null(全 `todo!()`) | 3 个测试失败 |
| 协变返回不装箱 | DOES NOT BUILD |
| 委托写成 `self.method(...)` | **存活** |

最后一个第一次跑时**我的变异自己没编译**(Python 里多写了 `\$`),
按老规矩重写后再跑——**不编译的变异不算数**。

重写后它**存活**:`self.method(...)` 和 `Type::method(self, ...)` 行为完全一样,
因为 Rust 优先解析固有方法。这是**等价变异**,如实记下。

显式路径保留,理由是它防的是固有方法**缺席**时解析到 trait 方法自己、
从而无限递归。但当前语料里构造不出这个情形
(`_matching` 找得到的方法,固有版本一定也生成了),
所以**这条防护测不出来**,不是测试杀死了它。

### 结果

`painting/`:`abstract method` 57 → 0,零拒绝的类 2 → 6。
**全 framework**:拒绝 7798 → **7012**,零拒绝的类 347 → **438**。
测试 24 个全过,其中 4 个走 `dyn AlignmentGeometry` 动态分发。

---

## 第 6 轮:`super`,以及一个必须更正的记录

### 更正:`package:kernel` 一直都在

README 里"Kernel 取不到"那段理由**是错的**,现在已改。它在:

    E:/source/flutter/engine/src/flutter/third_party/dart/pkg/kernel/

我上次搜的是 `engine/src/third_party/`,而它在 `engine/src/**flutter**/third_party/dart`——
从 `E:/source/flutter` 算是第 7 层,我用了 `-maxdepth 6`,**正好差一层**,
然后据此写下了"取不到"。是用户指出 upstream 源码在 `engine/src` 才发现的。

验证过它能用:拿 `pkg/kernel` 读 SDK 缓存里的 `dart2js_platform.dill`,
报 `Unexpected Kernel Format Version 140 (expected 139)`——
**说明它读到了文件、解析了头,只是版本差一个**。
改读同一 checkout 里修订版匹配的
`engine/src/out/host_release/flutter_patched_sdk/platform_strong.dill`:

    libraries: 20   classes: 1374   procedures: 748
    sample: AsyncError (abstract=false, super=Object), ...

**Kernel 管线是通的**,而且同一 checkout 里有修订版匹配的
`frontend_server_aot.dart.snapshot`,能产出配套的 app.dill。

用户还指出了更重要的一点:**要发布只能走工具链构建好的 app.dill**。
这是对的——app.dill 是**整个程序**,链接完毕、可达性确定、常量已求值、
mixin 已展开,而按文件翻译源码永远只是在逼近它。

### 本轮做的仍然是 `super`(692),而且它换前端也不作废

Rust **没有 `super`**。一旦 impl 覆盖了 trait 的默认方法,默认实现就不可达了——
`Trait::name(self)` 会派发回覆盖版。而"在覆盖 `X` 的方法里调 `super.X()`"
**不是边缘情况**:painting/ 和 rendering/ 里 435 个 super 调用**全是**这个形状。

所以每个抽象类的有实现方法生成**两份**:
一个自由泛型函数持有函数体,trait 的默认方法调它。
`super.X(..)` 就指那个函数——**唯一一个不可能派发到别处的东西**。

Kernel 前端下这件事只会更容易(Kernel 的 super 调用已经解析到具体目标),
Rust 这一侧的问题一模一样,所以这一轮的工作 100% 转移。

### 两个粒度 bug,同一个教训

1. **后端按类拒绝,前端按成员拒绝。** super 通了以后,`Alignment.add`
   不再因 super 被拒,改为因旁边的 `is` 被拒——**整个 `Alignment` 类因此消失**。
   现在后端也按成员拒绝,拒绝的那个成员在输出里留 `// NOT TRANSLATED` 和原因。
2. 更早一步:`emitLibrary` 一个类失败就丢掉整个文件。也改成按类。

**拒绝的粒度应该等于工作的粒度。** 两次都是这条。

### 还有一个只有 `dyn` 才能发现的 bug

`_emitBaseImpl` 原来只为**抽象**方法生成 override。
子类覆盖基类的**有实现**方法时,impl 里没有这一项,
于是走 `dyn Base` 会调到 trait 默认实现——**固有方法仍然是对的**,
所以只有穿过 `dyn` 的调用能看出来。测试因此全部走 `dyn`。

### 变异:四个,三杀一等价

| 变异 | 结果 |
|---|---|
| `super` 降成 `self.method(..)` | **栈溢出**,两个测试崩溃 |
| 覆盖基类有实现方法时不进 impl | 2 个测试失败 |
| 对未翻译的基类方法照样生成 super 调用 | DOES NOT BUILD |
| trait 默认方法内联 body 而不委托 | **存活** |

第一个我的脚本报的是 `NO RESULT`,单独跑确认了是
`thread ... has overflowed its stack`——**是杀死,不是无结果**,查清楚才记。

第三个第一次跑时**存活**,因为语料里没有这种情况。补了 fixture
(基类方法用了未支持的级联)之后它才被杀。**测不到的防护等于没有防护。**

第四个是**真等价**:自由函数照样生成,`super` 走的是它,
内联与否只是重复一份函数体,不改行为。

### 结果

测试 27 个全过,其中 3 个穿 `dyn Shape` 验 super。

---

## 第 7 轮:gallery 的 app.dill 造出来了,而且读得动

上一轮定的:**先造 dill 并确认能读出 gallery 自己的类,再动前端**——
造不出来的话,整条路走不通,要在写代码之前就知道。

### 结果:通了

    libraries: 1317   classes: 7467

| 来源 | 类数 |
|---|---|
| `package:flutter` | 4281 |
| `dart:*` | 1374 |
| **`package:gallery`** | **689** |
| `package:flutter_localizations` | 363 |
| `package:get` | 208 |

105 MB,frontend_server 零错误退出。

### Kernel 已经替我们做完的事(analyzer 前端要自己做的)

| | |
|---|---|
| 已展开的匿名 mixin 应用 | **871** |
| gallery 里的 super 调用,**目标已解析** | **285**(`initState -> State.initState`) |
| gallery 里已求值的 const 字段 | **273** |

第 6 轮花了一整轮解决 super 调用要指向谁的问题;Kernel 里它**本来就带着目标**。
871 个 mixin 应用在 analyzer 前端下是 871 次要自己做的类层次推导。

### 修订版匹配是这件事的全部难点

Flutter SDK 的 `bin/cache/dart-sdk` 和引擎里的 Dart checkout **是不同修订版**,
而 Kernel 二进制格式带版本号。用一边的 `pkg/kernel` 读另一边产出的 dill:

    Unexpected Kernel Format Version 140 (expected 139)

引擎 checkout 里恰好有**完整的一套同修订版工具**——
它自己编出来的 `dart-sdk`、`dartaotruntime`、`frontend_server` 快照、
patched platform、`pkg/kernel`,全在 `bb17f25f...`。
所以工具里的每一条路径都取自那一棵树,**一条都不来自 Flutter SDK**。

这就是 `bin/dill.py` 存在的理由,而不是把命令写进 README:
路径的修订版一致性是个静默失败的陷阱,
`--check` 会把八个位置和 revision 一次列清楚。

### 工具

    python tools/dart2rust/bin/dill.py --check
    python tools/dart2rust/bin/dill.py --config <out.json> [--config-root <dir>]
    python tools/dart2rust/bin/dill.py --build package:gallery/main.dart \
        --packages <app>/.dart_tool/package_config.json -o app.dill

    dart run --packages=<out.json> tools/dart2rust/bin/dill_info.dart app.dill

`--build` 是**用提交进仓库的工具重跑过一遍**的(105.1 MB,exit 0),
不是把 scratchpad 里手敲的命令抄进文件——没跑过的工具不算验证。

一个细节记下:frontend_server **编译出错也可能 exit 0**,
所以 `build()` 另外扫 stdout 里的 `Error:` 行,不只看退出码。

---

## 第 8 轮:Kernel 前端,两个前端产出一致

验收标准是上一轮定死的:**`ir.dart`、`backend_rust.dart`、27 个测试一行不改**,
Kernel 前端产出的 Rust 要让它们全过。

**达到了。**

    analyzer  ok. 27 passed
    kernel    ok. 27 passed

    212 行代码 vs 212 行,22 行不同

而且那 22 行**全是同一个外观差异**:analyzer 产 `Alignment::new((-1.0), (-1.0))`
(它看到的是 `IrUnary('-', 1.0)`),Kernel 产 `Alignment::new(-1.0, -1.0)`
(常量求值直接给出负数)。两个都对,Kernel 的更干净。

### 先 dump 节点,再写代码

Kernel 是脱糖过的,照 Dart 源码猜它的形状正是要避免的错误。
所以先写了个探针把 `Alignment` 里**真正出现**的节点种类全印出来,照着写:

- 算术**全是方法调用**:`x - other.x` 是 `InstanceInvocation(x, '-', [other.x])`,
  `this.{Alignment.+}(other)` 也是。要**映射回二元表达式**,
  否则会生成 `a.add(b)`——既不是上游写的,也不是后端运算符 trait 期待的。
  这一步是两个前端 IR 能对上的关键。
- 运算符方法是 `Procedure(kind: Operator)`,名字就是 `+`、`unary-`、`~/`
- 字段读是 `InstanceGet`,`this.{Alignment.x}`
- 局部变量声明是 `VariableStatement` 包着 `VariableDeclaration`
- 名字访问器是 `cosmeticName`(人写的名字;CFE 造的临时变量没有,
  或者以 `#` 开头)

### Kernel 白给的东西

- **super 调用带着已解析的目标**(`node.interfaceTarget.enclosingClass`)——
  第 6 轮整轮在解决的问题,这里是现成的
- **常量已求值**:`const Alignment(-1, -1)` 到手就是类加字段值。
  重建成构造函数调用而不是结构体字面量,好让产出的 Rust 仍然读作
  `Alignment::new(-1.0, -1.0)`——值一样,而源码形态还认得出
- **默认参数值是表达式**,不像 analyzer 那边只能读源码文本再认字面量
- 匿名 mixin 应用会被跳过(`isAnonymousMixin`),父类往上找到第一个真类

### diff 抓到的一个真 bug

`TextAlignVertical::new()`——**少了参数**。它的构造函数是
`const TextAlignVertical({required this.y})`,而我的常量处理只走了
`positionalParameters`。位置参数**和**具名参数都要走,顺序和
`_lowerConstructor` 一致。

**这个 bug 是 diff 发现的,不是测试**:两个前端的输出摆在一起才看得出来。

### 真实损失:Kernel 里没有文档注释

analyzer 版 492 行,Kernel 版 299 行,差的几乎全是 doc comment——
dill 里不带它们。这是这条路要付的代价,记在这里而不是发现时再惊讶。

### `agree.py`:把"两个前端一致"变成可重复的检查

    python tools/dart2rust/bin/agree.py --dill <app.dill>

它两个前端各生成一次、各跑一遍完整测试、再 diff 代码行(去掉注释,
否则会被 analyzer 那一侧的 doc 淹没),最后把工作树还原。

**做过一次的事和能重复的检查不是一回事**——这一条以后每轮都能跑。

---

## 第 9 轮:翻译私有成员,拒绝数涨了 70%

这一轮作废一条规则:**两个前端都在跳过私有成员**。
按文件翻译时那是对的——库外没人能叫它的名字。
整程序视角下**是错的**,而且错在要害:Flutter 每个 StatefulWidget 的实现
都在私有 State 类里,gallery 自己的 689 个类同理。

预测过会大涨,涨了:

|  | 之前 | 之后 |
|---|---|---|
| 类 | 1931 | **3331** |
| 拒绝 | 7012 | **11869** |
| 已生成成员 | 15313 | **21095** |

**这不是退步,是尺子第一次量到真东西。** 之前那个低拒绝数有一部分是
靠"看得少"换来的——`CupertinoApp` 报零拒绝,因为它的行为全在
`_CupertinoAppState` 里,而普查连那个类都没看。普查自己也有一条
私有类过滤,一并去掉了。

### Rust 侧怎么表达 Dart 的私有

Dart 的私有是**按库**的,Rust 的是**按模块**的,而一个文件的类正好
生成到一个模块里——所以 `_` 前缀的成员不加 `pub` 就得到同样的可达性。
**名字不动**,`_x` 本来就是合法的 Rust 标识符,改名会让输出没法和上游对照搜。

### 私有成员一进来,四个真 bug 就掉出来了

1. **Dart getter 不是 Rust 字段。** `AlignmentGeometry._x` 是抽象 getter,
   变成 trait 方法,当字段读就是
   "attempted to take value of method `_x`"。这笔账第 4 轮就记下了,现在必须还。
   - Kernel 直接说了是 `Field` 还是 `Procedure`;
   - **analyzer 有个陷阱**:它把**字段**读也解析成 `PropertyAccessorElement`
     (字段的隐式访问器)。所以"是不是 PropertyAccessorElement"问错了,
     要问的是 **`isSynthetic`**——隐式的是合成的,真 getter 不是。
2. **`operator ==` 生成了 `op_61_61`**,`superFn` 生成了
   `alignment_geometry_super_`(空名字)。运算符名字必须是合法标识符,
   而且要可读:现在每个 Dart 运算符都有名字,认不出的**停下**而不是写成十进制。
3. **`Alignment._stringify(...)` 被调用但从未生成**——第 1 轮那个 bug
   换形态回来了。当时我用"拒绝一切私有引用"挡住它,那条钝规则一撤,它就回来了。
   正确的规则和 `_superCall` 同形:**被调方在本文件里,就必须在 IR 里**。
4. **具体类型返回到抽象返回位置要装箱**。
   `AlignmentGeometry.add` 结尾是 `_MixedAlignment(...)`,
   声明返回 `AlignmentGeometry` = `Box<dyn AlignmentGeometry>`。
   这是第 5 轮协变返回问题在**函数体里**的版本。
   只对 `IrNew` 装箱——只有构造函数调用**确知**产出那个具体类型,
   别的可能已经是 box,多包一层会静默出错。

另外 analyzer 前端把顶层函数 `clampDouble(a,b,c)` 当成了 `self.clamp_double(...)`。
Kernel 前端一直是拒绝顶层调用的,**两边不一致,而且 analyzer 那边是错的**。

### 粒度教训的第三次

trait 那条路当时没跟上"按成员拒绝"的改动:一个 `toString` 里的字符串拼接
就把整个 `AlignmentGeometry` trait 带走,它的每个 impl 都编译不了。
现在 trait 的必需方法、默认方法、super 自由函数**全部按成员保护**。

配套一条:自由函数没生成时,trait 的对应默认方法不能去调它,
改成 `todo!()`——trait 和它的每个 impl 仍然对得上,少一个方法就对不上了。

### 桩反而变忠实了

`Offset.dx`、`Size.width`、`Rect.width/height`、`RRect.tlRadius`
上游**本来就是 getter**,桩里一直当字段。编译器分得清之后,桩改成方法,
**比之前更接近上游**。而 `Rect.left/top/right/bottom` 在 dart:ui 里
**确实是字段**——我一度把它们也改成 getter,那是改过头,查了 dart:ui 才改回来。

### 结果

    analyzer  ok. 27 passed
    kernel    ok. 27 passed
    AGREE

339 / 345 行,差 28 行。两个前端仍然一致。

---

## 第 10 轮:先把"赋值"这一条拆开,再做不需要可变性模型的那一半

上一轮留的话是"这个数字本身要先被信任"。查了,它确实是几件事。

Kernel 把赋值分成不同节点类型,所以拆分只是个计数:

| `package:flutter`(664 库,20922 次赋值) | |
|---|---|
| `VariableSet`(局部变量) | **10633(51%)** |
| `InstanceSet`(字段或 setter) | 10089(48%) |
| `StaticSet` | 188 |
| `SuperPropertySet` | 12 |

字段写里 6220 次穿过 `this`,3869 次穿过别的对象。
gallery 那边同样对半(700 / 667)。

**一半只需要 `let mut`,另一半才需要 `&mut self` 和"可变性怎么传染"的故事。**
照合并后的数字去规划,等于给两倍于实际需要的地方做难的那套。

本轮只做局部赋值。

### `let mut` 是关于整个函数体的事实

Rust 要在**声明处**写 `mut`,而需不需要是**整个函数体**的性质,不是那一行的。
所以生成前先走一遍函数体,收集被赋值的局部名。

参数也一样:Dart 的参数就是普通变量、随便重新赋值,
Rust 的参数默认不可变,`mut start: f32` 是说这件事的地方。
没有它,`shadow(start) { start = start + 1; }` 会生成一个赋不了值的赋值。

顺带:`_reassigned` 必须在**写签名之前**算好——签名先出来,参数的 `mut` 在签名里。

### 两个前端要在复合赋值上会合

analyzer 把 `x += 1` 保留成一个节点,Kernel 已经改写成 `x = x + 1`。
两边必须到达同样的 IR,所以 analyzer 那侧在前端就展开。

fixture 里 `total += step; total -= 1.0; total *= 2.0` 的**顺序**是故意的:
减法夹在中间,这样"`x *= 2` 展开成 `x = 2 * x`"这种错法也会被抓到——
只有加法和乘法的话,交换律会让错的展开也得出对的数。

### 一个变异存活,于是把它变成可检验的

**"所有局部都标 mut"存活了**——它也能编译、所有测试也都过,
差别只是一堆警告。这一点 fixture 自己的注释里就预言过。

所以给测试 crate 加了 `#![deny(unused_mut)]`:
**把"只给真正被重新赋值的局部标 mut"从一句风格主张,变成构建会检查的事。**
加了之后同一个变异 DOES NOT BUILD。

另一个变异("被重新赋值的参数不标 mut")第一次跑时**我写的 Dart 不合法**,
按老规矩重写——不编译的变异不算数。重写后也被杀死。

四个变异,全部杀死。

### 结果

    analyzer  ok. 31 passed
    kernel    ok. 31 passed
    AGREE

普查:`AssignmentExpressionImpl` 那一条消失了,
剩下 **1056 次"assignment to a field"**——这是它本来就该被分成的样子。

---

## 下一步:后缀 `!`(1294,新队头)

它便宜、孤立,而且现在是队头:`b!` 就是 `b.unwrap()`。
要当心一处:Dart 的 `!` 断言非空,Rust 的 `unwrap()` 会 panic ——
**语义一致**(两者都是"我保证它不是 null,不是就崩"),
但 panic 信息不同,而且 `unwrap` 在 release 里也在。
上游用 `!` 的地方是它自己保证过的,照抄即可,别改成 `unwrap_or_default`。

## 之后:字段赋值 + setter(1056 + 812 = 1868)

这才是可变性那件大事,单独规划。三个要先回答的问题:

1. 方法写字段就要 `&mut self`,而 `&mut` 会传染到调用者。**谁判断传染边界?**
2. `impl Add for Alignment` 这类 trait 方法签名固定(`self`),里面有赋值怎么办?
3. Dart 的 `a.x = 1`,如果 `x` 有 setter,要变成 `a.set_x(1)`——
   **这需要知道 `x` 有没有 setter**,是跨类的信息。

已量到的分布可以直接用:字段写里 **6220 次穿过 `this`**(方法内改自己),
**3869 次穿过别的对象**(改别人)。第一类是 `&mut self`,
第二类要 `&mut` 的接收者,难得多。**先做第一类。**

## 当前队头(685 文件 / 3331 个类 / 11837 次拒绝)

| 次数 | 要建的东西 |
|---|---|
| 1294 | 后缀 `!` |
| 1056 | 字段赋值 |
| 812 | setter |
| 725 | 枚举值引用 |
| 725 | 闭包字面量 |
| 535 | 调用函数值 |
| 473 | getter 引用(未解析) |
| 411 | super 构造函数调用 |

## 两条要记住的警告

1. **"零拒绝"(778 / 3331)不等于"翻译好了"**:引用其他库的东西
   (`Object.hash`、`dart:ui` 的类型)仍然靠手写桩顶着。
2. **`testdata` 的桩已经吃力**。下一个会撞上的墙是:
   翻译一个类要连带它依赖的整个图,而手写桩不可能跟上。
   真正的答案是**翻译 `dart:core` 和 `dart:ui` 的最小子集**,
   那时候 `agree.py` 的对照会变成整程序级的。
