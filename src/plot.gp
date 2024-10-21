set terminal pngcairo size 800,600
set output "fft.png"

set title "Freq vs Power(dB)"

set xlabel "Freq (index)"
set ylabel "Power (dB)"

set grid

# set xrange [0: 1024]

plot "output.dat" using 1:2 with linespoints title "Power"
     
set output
