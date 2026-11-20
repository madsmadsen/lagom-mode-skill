(ns cov.quiet
  "Cloverage entry point that keeps stdout stable for the parity run.

  Cloverage prints an elapsed time and a download notice, and clojure.test
  prints its progress. The parity runner compares the stdout of the original
  with the stdout of the port, so this entry point sends that text to stderr.
  The report files are written as usual. A failed run prints the captured text
  on stderr and exits with the status of cloverage."
  (:require [clojure.test]
            [cloverage.coverage]))

(defn -main [& args]
  (let [captured (java.io.StringWriter.)
        writer (java.io.PrintWriter. captured)
        code (try
               (binding [*out* writer
                         *err* writer
                         clojure.test/*test-out* writer
                         cloverage.coverage/*exit-after-test* false]
                 (apply cloverage.coverage/-main (concat (vec args) ["--lcov"])))
               (catch Throwable error
                 (.printStackTrace error writer)
                 -1))]
    (.flush writer)
    (binding [*out* *err*] (println (str captured)))
    (shutdown-agents)
    (System/exit (int (min (max (long code) -128) 127)))))
