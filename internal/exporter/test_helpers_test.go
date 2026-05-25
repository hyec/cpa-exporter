package exporter

import dto "github.com/prometheus/client_model/go"

func hasMetricFamily(families []*dto.MetricFamily, name string) bool {
	for _, family := range families {
		if family.GetName() == name && len(family.GetMetric()) > 0 {
			return true
		}
	}
	return false
}

func metricFamilyByName(families []*dto.MetricFamily, name string) *dto.MetricFamily {
	for _, family := range families {
		if family.GetName() == name {
			return family
		}
	}
	return nil
}

func metricHasLabel(metric *dto.Metric, name, value string) bool {
	for _, label := range metric.GetLabel() {
		if label.GetName() == name && label.GetValue() == value {
			return true
		}
	}
	return false
}

func metricHasLabelName(metric *dto.Metric, name string) bool {
	for _, label := range metric.GetLabel() {
		if label.GetName() == name {
			return true
		}
	}
	return false
}
