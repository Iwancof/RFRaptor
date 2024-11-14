set terminal pngcairo size 800,600
set output "iq.png"

set title "IQ vs. Count"

set xlabel "Count"
set ylabel "IQ (dB)"

set grid

set xrange [0: 1000]
set yrange [-0.1: 0.1]

plot "iq.dat" using 0:1 with lines title "real", \
     "iq.dat" using 0:2 with lines title "imag"

set output

set terminal pngcairo size 800,600
set output "freqency.png"

set title "Frequency vs. power"

set xlabel "Frequency (MHz)"
set ylabel "Power (dBm)"

set grid

set xrange [2416: 2435]
set yrange [0: 100]

plot "freq.dat" using 1:2 with lines title "Power"

set output

