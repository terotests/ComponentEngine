// hermes: regexp engine OOB in lookbehind / backreferences / case folding (Hermes, QuickJS, V8 Irregexp) regexp
var cases = [
  [/(?<=(\d+)(\d+))$/, '1053'], [/(?<=\1(a))b/, 'aab'], [/(?<!(^|[ab]))\w{2}/, 'abcdef'],
  [/(?<=^(?:a|ab)*)c/, 'ababc'], [/(a)|\1b/, 'b'], [/\k<x>(?<x>a)/, 'aa'], [/(?<=\u{10400})x/iu, '\u{10428}x'],
  [/[\u{10400}-\u{1044f}]/iu, '\u{10428}'], [/(?<=a{0,65535})b/, 'aaab'], [/(?:(?=(a))\1)+b/, 'aaab'],
  [/^(?:\2(a)|(b))*$/, 'abab'], [/(?<a>.)(?<b>.)\k<b>\k<a>/, 'abba'], [/\b\B/u, 'a'], [/[^\W\d]/ui, 'ſ'],
];
for (var i = 0; i < cases.length; i++) {
  try { var m = cases[i][0].exec(cases[i][1]); print(i, m ? m.index + ':' + m.join('|') : 'null'); } catch (e) { print(i, e.name); }
}
try { print(new RegExp('(?<=' + 'a'.repeat(5000) + ')b').test('a'.repeat(5000) + 'b')); } catch (e) { print(e.name); }
