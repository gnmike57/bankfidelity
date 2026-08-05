#let data = json("data.json")

#set page(
  paper: "a4",
  margin: (x: 2cm, y: 2.5cm),
  header: [
    #text(14pt, weight: "bold", font: "Helvetica")[Bank Statement]
    #h(1fr)
    #text(10pt, font: "Helvetica")[Account: #data.account_number]
    #v(0.5cm)
    #line(length: 100%, stroke: 1pt + luma(150))
  ],
  footer: context [
    #set text(8pt, fill: luma(100), font: "Helvetica")
    #align(center)[Page #counter(page).display("1 of 1", both: true)]
  ]
)

#set text(font: "Helvetica", size: 10pt)
#set par(leading: 0.65em)

#v(1cm)
#grid(
  columns: (1fr, 1fr),
  [
    #text(12pt, weight: "bold")[Statement Summary]\
    Opening Balance: \$#data.opening_balance \
    Closing Balance: \$#data.closing_balance \
  ],
  align(right)[
    #text(10pt)[Statement Period: #data.period]
  ]
)

#v(1cm)

#table(
  columns: (1.2fr, 4.5fr, 1.2fr, 1.2fr, 1.5fr),
  stroke: (x, y) => if y == 0 { (bottom: 1pt + black) } else { none },
  align: (left, left, right, right, right),
  
  [*Date*], [*Description*], [*Debit*], [*Credit*], [*Balance*],
  
  ..data.transactions.map(tx => (
    tx.date,
    tx.description,
    if tx.debit != null { "$" + str(tx.debit) } else { "" },
    if tx.credit != null { "$" + str(tx.credit) } else { "" },
    if tx.balance != null { "$" + str(tx.balance) } else { "" }
  )).flatten()
)

#v(2cm)
#align(center)[
  #text(8pt, style: "italic", fill: luma(100))[End of Statement]
]
