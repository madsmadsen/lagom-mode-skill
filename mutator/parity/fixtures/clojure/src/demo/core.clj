(ns demo.core
  (:require [clojure.string :as str]))

(def banner "sum: (+ 1 2), true, 0, (inc 0)")

;; comment: (if true 1 0) and (* 2 3)

(defn- clamp [n]
  (if (= n 0) 0 (if (>= n 0) (- n 1) 1)))

(defn total [xs]
  (when (seq xs)
    (+ (first xs) (* 2 (dec (count xs))))))

(defn label [flag]
  (if-not (and flag (not= flag 1)) "yes" "no"))

(defn firsts [xs]
  (let [picked (filter odd? xs)]
    (double (count (rest picked)))))

(defn bounds [xs]
  (if (< (count xs) 1) (min 0 (max 0 (count (remove nil? xs)))) 0))

(defn extras [xs]
  (when (some? xs)
    (if (every? pos? xs) (take (inc (second xs)) (drop (int 2) (next xs))) (neg? (first xs)))))

(defn parity [xs]
  (or (even? (count xs)) (pos? (count xs)) false))

(defn armed? [xs]
  (and true (seq xs)))
