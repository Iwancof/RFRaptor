set terminal pngcairo size 800,600
set output "signal.png"

set title "Count vs Signal"

set xlabel "Count"
set ylabel "Signal"

set grid

set palette defined (0 "blue", 1 "red")
unset colorbox

set yrange [0:100]
set xrange [0:50000]

plot "signal.dat" using 1:2:3 with linespoints palette title "Signal"

set output
