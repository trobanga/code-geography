// Ground truth for the `calls` edges of the Java front end: javac parses and attributes
// the sources (with the annotation processors on the classpath, so Lombok runs) and this
// program prints one line per call site that is written in the source.
//
//   java CallTruth.java <repo root> <classpath file> <out.tsv> <scratch dir>
//
// The repo root is walked for `*.java` below a `src/` directory (hidden directories,
// `target` and `node_modules` are skipped, as cg-walk does). The classpath file holds one
// `:`-separated classpath. Diagnostics go to <out.tsv>.diag. Nothing is written into the
// repo: class and generated source output goes to <scratch dir>.
//
// Columns of <out.tsv> (tab-separated, one header line):
//   file start end line kind caller callee owner scope origin
//   recvKind recvSym recvDecl recvScope recvType static
// file: relative to the root; start/end: UTF-8 byte offsets of the call expression (the
// same as tree-sitter's node range); kind: call | super | this | new | new-anon | ref |
// enumconst; caller: our node id of the function or type that holds the site; callee: our
// id of the method or constructor (fn:Owner#name(written parameter types)); owner: the
// qualified name of the declaring type; scope: internal (declared in the sources) or
// external; origin (internal only): src | lombok | mandated | notree | local | lombok-type.

import com.sun.source.tree.*;
import com.sun.source.util.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.stream.*;
import javax.lang.model.element.*;
import javax.lang.model.type.*;
import javax.lang.model.util.Elements;
import javax.tools.*;

public class CallTruth {
  static Trees trees;
  static SourcePositions pos;
  static Elements elements;
  static Path root;
  static PrintWriter out;
  static Map<String, Integer> skipped = new TreeMap<>();

  public static void main(String[] args) throws Exception {
    root = Path.of(args[0]).toAbsolutePath().normalize();
    String classpath = Files.readString(Path.of(args[1])).trim();
    Path outFile = Path.of(args[2]);
    Path scratch = Path.of(args[3]);
    Files.createDirectories(scratch.resolve("classes"));
    Files.createDirectories(scratch.resolve("generated"));

    List<Path> files;
    try (Stream<Path> walk = Files.walk(root)) {
      files =
          walk.filter(p -> p.toString().endsWith(".java"))
              .filter(p -> isInput(root.relativize(p)))
              .sorted()
              .collect(Collectors.toList());
    }
    JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
    DiagnosticCollector<JavaFileObject> diagnostics = new DiagnosticCollector<>();
    StandardJavaFileManager fm = compiler.getStandardFileManager(diagnostics, null, StandardCharsets.UTF_8);
    List<String> options =
        List.of(
            "-classpath", classpath,
            "-processorpath", classpath,
            "--release", "21",
            "-encoding", "UTF-8",
            "-proc:full",
            "-implicit:none",
            "-Xmaxerrs", "100000",
            "-Xmaxwarns", "1",
            "-d", scratch.resolve("classes").toString(),
            "-s", scratch.resolve("generated").toString());
    JavacTask task =
        (JavacTask) compiler.getTask(null, fm, diagnostics, options, null, fm.getJavaFileObjectsFromPaths(files));
    trees = Trees.instance(task);
    pos = trees.getSourcePositions();
    elements = task.getElements();
    Iterable<? extends CompilationUnitTree> units = task.parse();
    task.analyze();

    int errors = 0;
    Map<String, Integer> errorFiles = new TreeMap<>();
    try (PrintWriter diag = new PrintWriter(Files.newBufferedWriter(Path.of(outFile + ".diag")))) {
      for (Diagnostic<? extends JavaFileObject> d : diagnostics.getDiagnostics()) {
        if (d.getKind() != Diagnostic.Kind.ERROR) continue;
        errors++;
        String f = d.getSource() == null ? "-" : root.relativize(Path.of(d.getSource().toUri())).toString();
        errorFiles.merge(f, 1, Integer::sum);
        diag.println(f + ":" + d.getLineNumber() + ": " + d.getMessage(Locale.ROOT).replace('\n', ' '));
      }
    }

    int units_n = 0;
    try (PrintWriter w = new PrintWriter(Files.newBufferedWriter(outFile))) {
      out = w;
      w.println(
          "file\tstart\tend\tline\tkind\tcaller\tcallee\towner\tscope\torigin\trecvKind\trecvSym\trecvDecl\trecvScope\trecvType\tstatic");
      for (CompilationUnitTree cu : units) {
        units_n++;
        new Scanner(cu).scan(cu, null);
      }
    }
    System.err.printf("files %d, compilation units %d, errors %d in %d files%n", files.size(), units_n, errors, errorFiles.size());
    errorFiles.forEach((f, n) -> System.err.println("  error file " + f + ": " + n));
    System.err.println("skipped (not written in the source): " + skipped);
  }

  static boolean isInput(Path relative) {
    for (Path part : relative) {
      String s = part.toString();
      if (s.startsWith(".") || s.equals("target") || s.equals("node_modules")) return false;
    }
    return relative.toString().contains("src/");
  }

  static class Scanner extends TreePathScanner<Void, Void> {
    final CompilationUnitTree cu;
    final String file;
    final String text;
    final int[] bytes; // UTF-8 byte offset of each char index
    final Map<Element, String> declStyle = new IdentityHashMap<>();
    final com.sun.source.tree.LineMap lines;

    Scanner(CompilationUnitTree cu) throws IOException {
      this.cu = cu;
      this.file = root.relativize(Path.of(cu.getSourceFile().toUri())).toString();
      this.text = cu.getSourceFile().getCharContent(true).toString();
      this.bytes = new int[text.length() + 1];
      for (int i = 0; i < text.length(); i++) {
        char c = text.charAt(i);
        int n = c < 0x80 ? 1 : c < 0x800 ? 2 : Character.isSurrogate(c) ? 2 : 3;
        bytes[i + 1] = bytes[i] + n;
      }
      this.lines = cu.getLineMap();
    }

    long start(Tree t) { return pos.getStartPosition(cu, t); }
    long end(Tree t) { return pos.getEndPosition(cu, t); }

    /** Whether the tree is written in the source (not generated by Lombok or javac). */
    boolean written(Tree t, String mustContain) {
      long s = start(t), e = end(t);
      if (s < 0 || e <= s || e > text.length()) return false;
      return mustContain == null || text.substring((int) s, (int) e).contains(mustContain);
    }

    void skip(String why) { skipped.merge(why, 1, Integer::sum); }

    @Override
    public Void visitVariable(VariableTree v, Void p) {
      Element e = trees.getElement(getCurrentPath());
      if (e != null) {
        Tree type = v.getType();
        String style;
        if (type == null || start(type) < 0) style = "implicit";
        else if (text.substring((int) start(type), (int) Math.max(start(type), end(type))).equals("var")) style = "var";
        else style = "explicit";
        Tree parent = getCurrentPath().getParentPath().getLeaf();
        if (parent instanceof LambdaExpressionTree) style = "lambda-" + style;
        else if (parent instanceof EnhancedForLoopTree) style = "for-" + style;
        else if (parent instanceof InstanceOfTree || parent instanceof BindingPatternTree) style = "pattern-" + style;
        declStyle.put(e, style);
      }
      return super.visitVariable(v, p);
    }

    @Override
    public Void visitMethod(MethodTree m, Void p) {
      Element me = trees.getElement(getCurrentPath());
      String name = m.getName().contentEquals("<init>") && me != null ? me.getEnclosingElement().getSimpleName().toString() : m.getName().toString();
      if (!written(m, null) || !CallTruth.declaresName(cu, m, name)) { skip("generated method"); return null; }
      return super.visitMethod(m, p);
    }

    @Override
    public Void visitClass(ClassTree c, Void p) {
      if (!written(c, null)) { skip("generated class"); return null; }
      return super.visitClass(c, p);
    }

    @Override
    public Void visitMethodInvocation(MethodInvocationTree t, Void p) {
      ExpressionTree sel = t.getMethodSelect();
      String name = sel instanceof MemberSelectTree ms ? ms.getIdentifier().toString() : ((IdentifierTree) sel).getName().toString();
      String kind = name.equals("super") ? "super" : name.equals("this") ? "this" : "call";
      if (!written(t, name)) skip("generated " + kind);
      else emit(t, kind, trees.getElement(getCurrentPath()), sel instanceof MemberSelectTree ms ? ms.getExpression() : null);
      return super.visitMethodInvocation(t, p);
    }

    @Override
    public Void visitNewClass(NewClassTree t, Void p) {
      Tree parent = getCurrentPath().getParentPath().getLeaf();
      boolean enumConstant = parent instanceof VariableTree
          && trees.getElement(getCurrentPath().getParentPath()) != null
          && trees.getElement(getCurrentPath().getParentPath()).getKind() == ElementKind.ENUM_CONSTANT;
      if (enumConstant) {
        if (!written(parent, null)) skip("generated enumconst");
        else emitAt(start(parent), end(parent), "enumconst", trees.getElement(getCurrentPath()), null);
      } else if (!written(t, "new")) {
        skip("generated new");
      } else if (t.getClassBody() != null) {
        emit(t, "new-anon", anonymousSuperConstructor(t), null);
      } else {
        emit(t, "new", trees.getElement(getCurrentPath()), null);
      }
      return super.visitNewClass(t, p);
    }

    /** The constructor that an anonymous class creation reaches: the one its constructor calls. */
    Element anonymousSuperConstructor(NewClassTree t) {
      for (Tree member : t.getClassBody().getMembers()) {
        if (member instanceof MethodTree m && m.getName().contentEquals("<init>") && m.getBody() != null) {
          for (StatementTree s : m.getBody().getStatements()) {
            if (s instanceof ExpressionStatementTree es && es.getExpression() instanceof MethodInvocationTree mi) {
              Element e = trees.getElement(TreePath.getPath(cu, mi));
              if (e != null) return e;
            }
          }
        }
      }
      // An interface: the anonymous class extends Object.
      return null;
    }

    @Override
    public Void visitMemberReference(MemberReferenceTree t, Void p) {
      if (!written(t, "::")) skip("generated ref");
      else emit(t, "ref", trees.getElement(getCurrentPath()), t.getQualifierExpression());
      return super.visitMemberReference(t, p);
    }

    void emit(Tree t, String kind, Element callee, ExpressionTree receiver) {
      emitAt(start(t), end(t), kind, callee, receiver);
    }

    void emitAt(long s, long e, String kind, Element callee, ExpressionTree receiver) {
      String caller = callerId(getCurrentPath());
      String calleeId = "-", owner = "-", scope = "external", origin = "-", isStatic = "0";
      if (callee instanceof ExecutableElement ex) {
        TypeElement ownerType = (TypeElement) ex.getEnclosingElement();
        owner = ownerType.getQualifiedName().toString();
        isStatic = ex.getModifiers().contains(Modifier.STATIC) ? "1" : "0";
        TreePath ownerPath = trees.getPath(ownerType);
        if (ownerPath != null) {
          scope = "internal";
          if (owner.isEmpty()) {
            origin = "local";
            owner = "<local>" + ownerType.getSimpleName();
          } else if (!writtenIn(ownerPath.getCompilationUnit(), ownerPath.getLeaf())) {
            origin = "lombok-type";
          } else {
            TreePath mp = trees.getPath(ex);
            if (mp == null || !(mp.getLeaf() instanceof MethodTree m)) {
              origin = "notree";
            } else if (!writtenIn(mp.getCompilationUnit(), m) || !declaresName(mp.getCompilationUnit(), m, ex)) {
              origin = elements.getOrigin(ex) == Elements.Origin.MANDATED ? "mandated" : "lombok";
            } else {
              origin = "src";
            }
          }
          if (origin.equals("src")) {
            TreePath mp = trees.getPath(ex);
            calleeId = functionId(owner, (MethodTree) mp.getLeaf(), mp.getCompilationUnit(), ex);
          } else {
            String n = ex.getKind() == ElementKind.CONSTRUCTOR ? ownerType.getSimpleName().toString() : ex.getSimpleName().toString();
            calleeId = "fn:" + owner + "#" + n + "(?" + ex.getParameters().size() + ")";
          }
        } else {
          origin = "-";
        }
      } else if (callee == null && kind.equals("new-anon")) {
        owner = "java.lang.Object";
      } else {
        owner = "?unattributed";
        scope = "unattributed";
      }
      String recvKind = "-", recvSym = "-", recvDecl = "-", recvScope = "-", recvType = "-";
      if (receiver != null) {
        recvKind = receiver.getKind().toString();
        TreePath rp = TreePath.getPath(cu, receiver);
        if (rp != null) {
          Element re = trees.getElement(rp);
          if (re != null) {
            recvSym = re.getKind().toString();
            recvDecl = declStyle.getOrDefault(re, "-");
          }
          TypeMirror tm = trees.getTypeMirror(rp);
          if (tm != null) {
            recvType = tm.toString().replace('\t', ' ');
            Element te = tm.getKind() == TypeKind.DECLARED ? ((DeclaredType) tm).asElement() : null;
            if (re != null && (re.getKind().isClass() || re.getKind().isInterface())) te = re;
            recvScope = te == null ? tm.getKind().toString() : trees.getPath(te) != null ? "internal" : "external";
          }
        }
      }
      long line = s >= 0 ? lines.getLineNumber(s) : -1;
      out.println(String.join("\t", file, String.valueOf(bytes[(int) s]), String.valueOf(bytes[(int) e]), String.valueOf(line),
          kind, caller, calleeId, owner, scope, origin, recvKind, recvSym, recvDecl, recvScope, recvType, isStatic));
    }

    /** Lombok gives some generated methods the position of the field or annotation they come from. */
    boolean declaresName(CompilationUnitTree unit, MethodTree m, ExecutableElement ex) {
      return CallTruth.declaresName(unit, m, ex.getKind() == ElementKind.CONSTRUCTOR
          ? ex.getEnclosingElement().getSimpleName().toString() : ex.getSimpleName().toString());
    }

    boolean writtenIn(CompilationUnitTree unit, Tree t) {
      long s = pos.getStartPosition(unit, t), e = pos.getEndPosition(unit, t);
      return s >= 0 && e > s;
    }

    /**
     * Our caller: the innermost method of a named member type chain that holds the site; a
     * site in a field initializer, an initializer block or an enum constant belongs to the
     * type; lambdas, anonymous and local classes belong to the enclosing function.
     */
    String callerId(TreePath path) {
      List<Tree> chain = new ArrayList<>();
      for (TreePath p = path; p != null; p = p.getParentPath()) chain.add(0, p.getLeaf());
      String owner = null; // qualified name of the current named type
      String result = null;
      for (int i = 0; i < chain.size(); i++) {
        Tree t = chain.get(i);
        Tree parent = i > 0 ? chain.get(i - 1) : null;
        boolean member = parent instanceof CompilationUnitTree || parent instanceof ClassTree;
        if (t instanceof ClassTree && member) {
          Element e = trees.getElement(pathOf(path, chain, i));
          owner = ((TypeElement) e).getQualifiedName().toString();
          result = "type:" + owner;
        } else if (parent instanceof ClassTree && t instanceof MethodTree m) {
          return functionId(owner, m, cu, (ExecutableElement) trees.getElement(pathOf(path, chain, i)));
        } else if (parent instanceof ClassTree) {
          return "type:" + owner; // field, initializer block, enum constant
        }
      }
      return result;
    }

    static TreePath pathOf(TreePath leaf, List<Tree> chain, int i) {
      TreePath p = leaf;
      for (int k = chain.size() - 1; k > i; k--) p = p.getParentPath();
      return p;
    }
  }

  /** Whether the source text of the method declares its name: `name(` or, compact, `name {`. */
  static boolean declaresName(CompilationUnitTree unit, MethodTree m, String name) {
    try {
      String text = unit.getSourceFile().getCharContent(true).toString();
      long s = pos.getStartPosition(unit, m), e = pos.getEndPosition(unit, m);
      if (s < 0 || e <= s) return false;
      return java.util.regex.Pattern.compile("\\b" + java.util.regex.Pattern.quote(name) + "\\s*[({]")
          .matcher(text.substring((int) s, (int) e)).find();
    } catch (IOException x) {
      throw new UncheckedIOException(x);
    }
  }

  /** Our id: fn:<qualified type>#<name>(<parameter types as written, without whitespace>). */
  static String functionId(String owner, MethodTree m, CompilationUnitTree unit, ExecutableElement ex) {
    String text;
    try {
      text = unit.getSourceFile().getCharContent(true).toString();
    } catch (IOException e) {
      throw new UncheckedIOException(e);
    }
    boolean constructor = m.getName().contentEquals("<init>");
    String name = constructor ? owner.substring(owner.lastIndexOf('.') + 1) : m.getName().toString();
    // A compact canonical constructor has no parameter list in the source.
    long ms = pos.getStartPosition(unit, m);
    long bodyStart = m.getBody() == null ? pos.getEndPosition(unit, m) : pos.getStartPosition(unit, m.getBody());
    String head = text.substring((int) ms, (int) bodyStart);
    if (constructor && !head.contains("(")) return "fn:" + owner + "#" + name + "()";
    List<String> types = new ArrayList<>();
    List<? extends VariableTree> ps = m.getParameters();
    for (int i = 0; i < ps.size(); i++) {
      Tree type = ps.get(i).getType();
      String written = text.substring((int) pos.getStartPosition(unit, type), (int) pos.getEndPosition(unit, type)).replaceAll("\\s+", "");
      if (ex != null && ex.isVarArgs() && i == ps.size() - 1) {
        if (written.endsWith("...")) written = written.substring(0, written.length() - 3);
        else if (written.endsWith("[]")) written = written.substring(0, written.length() - 2);
        written = written + "...";
      }
      types.add(written);
    }
    return "fn:" + owner + "#" + name + "(" + String.join(",", types) + ")";
  }
}
