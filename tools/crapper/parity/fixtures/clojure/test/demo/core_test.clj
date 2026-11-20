(ns demo.core-test
  (:require [clojure.test :refer [deftest is]]
            [demo.core :as core]
            [demo.shared :as shared]))

(deftest grades-scores
  (is (= :a (core/grade 95)))
  (is (= :c (core/grade 10))))

(deftest describes-a-named-item
  (is (= "book" (core/describe-item {:name "book"}))))

(deftest tags-a-value
  (is (= "42" (shared/tag 42))))
