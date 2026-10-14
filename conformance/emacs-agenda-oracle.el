(require 'cl-lib)
(require 'org)
(require 'org-agenda)

(let* ((case-dir (or (getenv "AGD_CASE_DIR")
                     (error "AGD_CASE_DIR is required")))
       (from (or (getenv "AGD_FROM")
                 (error "AGD_FROM is required")))
       (today (or (getenv "AGD_TODAY")
                  (error "AGD_TODAY is required")))
       (date-parts (mapcar #'string-to-number (split-string today "-")))
       (fixed-time (encode-time 0 0 12
                                (nth 2 date-parts)
                                (nth 1 date-parts)
                                (nth 0 date-parts)
                                t))
       (days (string-to-number (or (getenv "AGD_DAYS") "1"))))
  (setq org-agenda-files (directory-files case-dir t "\\.org\\'")
        org-agenda-start-day from
        org-agenda-start-on-weekday nil
        org-agenda-span days
        org-agenda-use-time-grid nil
        org-agenda-include-diary nil
        org-agenda-skip-scheduled-if-done
        (equal (getenv "AGD_SKIP_SCHEDULED_IF_DONE") "true")
        org-agenda-skip-deadline-if-done
        (equal (getenv "AGD_SKIP_DEADLINE_IF_DONE") "true")
        org-agenda-skip-timestamp-if-done
        (equal (getenv "AGD_SKIP_TIMESTAMP_IF_DONE") "true"))
  (when (getenv "AGD_SORTING")
    (setq org-agenda-sorting-strategy
          `((agenda ,@(mapcar #'intern (split-string (getenv "AGD_SORTING")))))))
  ;; org-today reads the clock through the time-since primitive, which a
  ;; current-time override does not reach, so freeze both.
  (cl-letf (((symbol-function 'current-time) (lambda () fixed-time))
            ((symbol-function 'org-today) (lambda () (time-to-days fixed-time))))
    (org-batch-agenda-csv "a")))
