// SPDX-License-Identifier: AGPL-3.0-or-later
//
// PDFKit document generation (pdfkit.mjs puts PDFKit's standalone build
// before this): PDF_PAGES pages of a heading, three justified paragraphs, a
// 12-row table with ruled, shaded rows and right-aligned numbers, a bar
// chart and a circle, Flate-compressed. Each of PDF_RUNS runs makes the
// whole document and ends at the stream's "end"; the best run is printed
// with the size and a hash of the bytes, which every engine must agree on.
var LOREM = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.";
function makePdf(pages, done) {
  var doc = new PDFDocument({ size: "A4", margin: 50, compress: true, info: { Title: "Benchmark", CreationDate: new Date(0) } });
  var bytes = 0, chunks = 0, hash = 0;
  doc.on("data", function (c) { bytes += c.length; chunks++; for (var i = 0; i < c.length; i += 97) hash = (hash * 31 + c[i]) | 0; });
  for (var p = 0; p < pages; p++) {
    if (p > 0) doc.addPage();
    doc.font("Helvetica-Bold").fontSize(18).fillColor("#1a3c6e").text("Section " + (p + 1) + ": Quarterly report", { underline: false });
    doc.moveDown(0.5);
    doc.font("Helvetica").fontSize(10).fillColor("black");
    for (var k = 0; k < 3; k++) doc.text(LOREM + " " + LOREM.slice(0, 40 + ((p * 7 + k * 13) % 120)), { align: "justify" }).moveDown(0.4);
    var y = doc.y + 6, x0 = 50, w = [140, 90, 90, 90, 85];
    doc.font("Helvetica-Bold").fontSize(9);
    var hdr = ["Item", "Q1", "Q2", "Q3", "Total"];
    var x = x0;
    for (var c = 0; c < 5; c++) { doc.text(hdr[c], x + 3, y + 3, { width: w[c] - 6, lineBreak: false }); x += w[c]; }
    doc.font("Times-Roman");
    for (var r = 0; r < 12; r++) {
      var ry = y + 16 * (r + 1);
      if (r % 2 === 0) doc.rect(x0, ry, 495, 16).fill("#eef2f8").fillColor("black");
      x = x0;
      var t = 0;
      for (c = 0; c < 5; c++) {
        var v = ((p + 3) * (r + 5) * (c + 7)) % 997;
        if (c > 0 && c < 4) t += v;
        var s = c === 0 ? "Product line " + (r + 1) : (c === 4 ? t : v).toFixed(2);
        doc.text(s, x + 3, ry + 4, { width: w[c] - 6, align: c === 0 ? "left" : "right", lineBreak: false });
        x += w[c];
      }
      doc.moveTo(x0, ry + 16).lineTo(x0 + 495, ry + 16).lineWidth(0.5).strokeColor("#99a").stroke();
    }
    var cy = y + 16 * 14 + 10;
    for (var b = 0; b < 8; b++) {
      var hgt = 20 + ((p * 11 + b * 17) % 60);
      doc.rect(70 + b * 55, cy + 80 - hgt, 35, hgt).fill(b % 2 ? "#4a7ab8" : "#e08a3c");
    }
    doc.circle(500, cy + 40, 30).lineWidth(2).stroke("#1a3c6e");
    doc.fillColor("black").font("Helvetica-Oblique").fontSize(8).text("Page " + (p + 1) + " of " + pages, 50, 780, { align: "center", width: 495, lineBreak: false });
  }
  doc.on("end", function () { done({ bytes: bytes, chunks: chunks, hash: hash }); });
  doc.end();
}
var PAGES = typeof PDF_PAGES === "number" ? PDF_PAGES : 10;
var RUNS = typeof PDF_RUNS === "number" ? PDF_RUNS : 3;
var best = 1e9, run = 0;
function next() {
  var t0 = performance.now();
  makePdf(PAGES, function (res) {
    var dt = performance.now() - t0;
    if (dt < best) best = dt;
    print("run " + run + ": " + dt.toFixed(1) + " ms");
    if (++run < RUNS) next();
    else print("pdfkit " + PAGES + " pages: " + res.bytes + " bytes, hash " + res.hash + ", best " + best.toFixed(1) + " ms");
  });
}
next();
