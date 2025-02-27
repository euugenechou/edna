import matplotlib
import matplotlib.pyplot as plt
import matplotlib.patches as mpatches
import csv
import statistics
import sys
import numpy as np
from textwrap import wrap

# plot styling for paper
matplotlib.rc('font', family='serif', size=11)
matplotlib.rc('text.latex',preamble='\\usepackage{inconsolata}\n\\usepackage[bitstream-charter]{mathdesign}\n\\usepackage{mathrsfs}')
matplotlib.rc('text', usetex=True)
matplotlib.rc('legend', fontsize=11)
matplotlib.rc('figure', figsize=(3,2.0))
matplotlib.rc('axes', linewidth=0.5)
matplotlib.rc('lines', linewidth=0.5)

colors=['g', 'y', 'm', 'w']
hatches=['','','', '////']
labels = ["Manual\n(No Edna)", "Direct\nDisguising",
          "Disguising Decorrelated\nData", "Crypto\nCost"]
fig = plt.figure(figsize=(5.5, .5))
patches = []
for (i, color) in enumerate(colors):
    patches.append(mpatches.Patch(edgecolor='black', facecolor=color, label=labels[i],
                                  hatch=hatches[i], ))
leg = fig.legend(patches, labels, mode='expand', ncol=4, loc='center', frameon=False,
           fontsize=11,handlelength=1)
for patch in leg.get_patches():
    patch.set_height(10)
plt.savefig("composition_legend.pdf", dpi=300)

plt.clf()
plt.figure(figsize = (3.2, 2.0))

def add_labels(x,y,plt,color,offset):
    for i in range(len(x)):
        if y[i] < 0.1:
            label = "{0:.1g}".format(y[i])
        elif y[i] > 100:
            label = "{0:.0f}".format(y[i])
        else:
            label = "{0:.1f}".format(y[i])
        new_offset = offset
        if y[i] < 50 or (y[i] < 80 and y[i] > 60 and color != 'm'):
            new_offset = (offset / 2.5)

        if y[i] > 34 and y[i] < 45 and color == 'black':
            new_offset += 10

        if y[i] > 300 or (y[i] > 30 and y[i] < 50) and color == 'm':
            new_offset += (y[i]/20)

        plt.text(x[i], y[i]+new_offset, label, ha='center', color=color, size=11)


def add_text_labels(x,y,plt,color,offset):
    for i in range(len(x)):
        plt.text(x[i], (offset/3), y[i], ha='center', color=color, size=11)

def get_yerr(durs):
    mins = []
    maxes = []
    for i in range(len(durs)):
        mins.append(statistics.median(durs[i]) - np.percentile(durs[i], 5))
        maxes.append(np.percentile(durs[i], 95)-statistics.median(durs[i]))
    return [mins, maxes]

barwidth = 0.25
# positions
X = np.arange(2)
labels = ['Remove\nAccount', 'Restore\nRem. Acct']

# WEBSUBMIT/HOTCRP RESULTS
for i in range(2):
    delete_durs_baseline = []
    delete_durs = []
    restore_durs = []
    delete_durs_noanon = []
    restore_durs_noanon = []
    delete_durs_dryrun = []
    restore_durs_dryrun = []
    delete_durs_dryrun_noanon = []
    restore_durs_dryrun_noanon = []

    filename_baseline ="../hotcrp_results/hotcrp_disguise_stats_3080users_baseline.csv"
    filename ="../hotcrp_results/hotcrp_disguise_stats_3080users.csv"
    filename_dryrun ="../hotcrp_results/hotcrp_disguise_stats_3080users_nocrypto.csv"
    title = "hotcrp"

    offset = 90
    if i == 0:
        filename_baseline ='../websubmit_results/disguise_stats_{}lec_{}users_baseline.csv'.format(20, 2000)
        filename = '../websubmit_results/disguise_stats_{}lec_{}users.csv'.format(20,2000)
        filename_dryrun = '../websubmit_results/disguise_stats_{}lec_{}users_dryrun.csv'.format(20,2000)
        title = "websubmit"
        offset = 18
        with open(filename,'r') as csvfile:
            rows = csvfile.readlines()
            delete_durs = [float(x)/1000 for x in rows[6].strip().split(',')]
            restore_durs = [float(x)/1000 for x in rows[7].strip().split(',')]
            delete_durs_noanon = [float(x)/1000 for x in rows[9].strip().split(',')]
            restore_durs_noanon = [float(x)/1000 for x in rows[10].strip().split(',')]

        with open(filename_dryrun,'r') as csvfile:
            rows = csvfile.readlines()
            delete_durs_dryrun = [float(x)/1000 for x in rows[6].strip().split(',')]
            restore_durs_dryrun = [float(x)/1000 for x in rows[7].strip().split(',')]
            delete_durs_dryrun_noanon = [float(x)/1000 for x in rows[9].strip().split(',')]
            restore_durs_dryrun_noanon = [float(x)/1000 for x in rows[10].strip().split(',')]

        with open(filename_baseline,'r') as csvfile:
            rows = csvfile.readlines()
            delete_durs_baseline = [float(x)/1000 for x in rows[6].strip().split(',')]
    else:
        with open(filename,'r') as csvfile:
            rows = csvfile.readlines()
            delete_durs = [float(x)/1000 for x in rows[3].strip().split(',')]
            restore_durs = [float(x)/1000 for x in rows[4].strip().split(',')]
            delete_durs_noanon = [float(x)/1000 for x in rows[6].strip().split(',')]
            restore_durs_noanon = [float(x)/1000 for x in rows[7].strip().split(',')]

        with open(filename_dryrun,'r') as csvfile:
            rows = csvfile.readlines()
            delete_durs_dryrun = [float(x)/1000 for x in rows[3].strip().split(',')]
            restore_durs_dryrun = [float(x)/1000 for x in rows[4].strip().split(',')]
            delete_durs_dryrun_noanon = [float(x)/1000 for x in rows[6].strip().split(',')]
            restore_durs_dryrun_noanon = [float(x)/1000 for x in rows[7].strip().split(',')]

        with open(filename_baseline,'r') as csvfile:
            rows = csvfile.readlines()
            delete_durs_baseline = [float(x)/1000 for x in rows[3].strip().split(',')]

    ################ add baseline closer to red line for anonymize
    plt.bar((X-barwidth)[:1], [statistics.median(delete_durs_baseline)],
            yerr=get_yerr([delete_durs_baseline]),
            error_kw=dict(capthick=0.5, ecolor='black', lw=0.5),color='g', capsize=3, width=barwidth, label="Manual (No Edna)", edgecolor='black', linewidth=0.25)
    add_labels((X-barwidth)[:1], [statistics.median(delete_durs_baseline)], plt, 'g', offset)
    add_text_labels((X-barwidth)[1:], ["N/A"], plt, 'g', offset)

    ############### edna w/out composition
    plt.bar((X), [
        statistics.median(delete_durs_noanon),
        statistics.median(restore_durs_noanon),
    ],
    yerr=get_yerr([
        delete_durs_noanon,
        restore_durs_noanon,
    ]),
    error_kw=dict(capthick=0.5, ecolor='black', lw=0.5),color='y', capsize=3,
            width=barwidth, label="Directly Sealing (Crypto)",
            edgecolor='black', linewidth=0.25, hatch = '/////')

    plt.bar((X), [
        statistics.median(delete_durs_dryrun_noanon),
        statistics.median(restore_durs_dryrun_noanon),
    ],
    color='y', capsize=0,
    width=barwidth, label="Directly Sealing (I/O)", edgecolor='black', linewidth=0.25)

    add_labels((X),
    [
        statistics.median(delete_durs_noanon),
        statistics.median(restore_durs_noanon),
    ], plt, 'black', offset)

    ############### edna w/composition
    plt.bar((X+barwidth), [
        statistics.median(delete_durs),
        statistics.median(restore_durs),
    ],
    yerr=get_yerr([
        delete_durs,
        restore_durs,
    ]),
            error_kw=dict(capthick=0.5, ecolor='black',
                          lw=0.5),color='m', capsize=3, width=barwidth,
            label="Sealing Decorrelated Data (Crypto)",edgecolor='black',
            linewidth=0.25, hatch='/////')

    plt.bar((X+barwidth), [
        statistics.median(delete_durs_dryrun),
        statistics.median(restore_durs_dryrun),
    ],
    color='m', capsize=0,
    width=barwidth, label="Sealing Decorrelated Data (I/O)",edgecolor='black', linewidth=0.25)

    add_labels((X+barwidth),
    [
        statistics.median(delete_durs),
        statistics.median(restore_durs),
    ], plt, 'm', offset)

    plt.ylim(ymin=0, ymax=650)
    plt.yticks(range(0, 650, 150))
    if i == 0:
        plt.ylim(ymin=0, ymax=100)
        plt.yticks(range(0, 100, 30))
        plt.ylabel('Time (ms)')
    plt.xticks(X, labels=labels)
    plt.subplots_adjust(left=0.25, right=1.0, bottom=0)
    plt.tight_layout(h_pad=0)
    plt.savefig('composition_stats_{}.pdf'.format(title), dpi=300)
    plt.clf()


    print(
        statistics.median(delete_durs_dryrun_noanon)/
        statistics.median(delete_durs_noanon),
        statistics.median(restore_durs_dryrun_noanon)/
        statistics.median(restore_durs_noanon),
        statistics.median(delete_durs_dryrun)/
        statistics.median(delete_durs),
        statistics.median(restore_durs_dryrun)/
        statistics.median(restore_durs))

